// arXiv on ChloeDB: the page, in ChloeDB's command center (#/app/arxiv,
// loaded through chloedb-server from this app) and on its own (the app's /
// with --public, through ui/index.html). It imports nothing and builds its
// own elements, so the same file serves both.
//
//   (home)              the box, today's papers, what is rising
//   search/<query>      papers, facets, the question answered, the query understood
//   paper/<id>          a paper: read next, cited with it, similar, references, cited by
//   author/<key>        a person: papers over time, coauthors, topics
//   topic/<n>           a topic of the atlas; concept/<slug> a technique or domain;
//   category/<code>     an arXiv category; venue/<name> a conference or journal
//   trends/<a,b,c>      topics side by side, year by year
//   hot                 what grows fastest
//   atlas               every topic on a map
//   foryou              what to read next, from what you opened and kept
//   library             what you kept
//   feeds, feed/<id>    your digests: the studio, its preview and its issues
//   issue/<id>          one issue, as a newsletter
//
// Every answer of the API carries the SQL it ran: the "SQL" button shows
// the queries behind the page in view.

export default { render };

const CSS = `
.ax { --ax-w: 1240px; color: var(--text); font: 14px/1.5 var(--sans); max-width: var(--ax-w); margin: 0 auto; }
.ax * { box-sizing: border-box; }
.ax a { color: inherit; text-decoration: none; }
.ax a:hover { text-decoration: underline; text-underline-offset: 2px; }
.ax mark { background: color-mix(in srgb, var(--warn) 34%, transparent); color: inherit; border-radius: 3px; padding: 0 1px; }
.ax button { font: inherit; }
.ax-top { position: sticky; top: 0; z-index: 5; background: color-mix(in srgb, var(--bg) 88%, transparent); backdrop-filter: blur(10px); padding: 12px 0 6px; }
.ax-bar { display: flex; align-items: center; gap: 14px; }
.ax-brand { display: flex; align-items: center; gap: 9px; font-weight: 680; letter-spacing: -.015em; white-space: nowrap; font-size: 15.5px; }
.ax-brand svg { width: 26px; height: 26px; color: var(--accent); fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.ax-brand small { font-weight: 500; color: var(--muted); font-size: 12px; }
.ax-box { position: relative; flex: 1; min-width: 0; }
.ax-box input { width: 100%; font: 15px var(--sans); padding: 10px 14px 10px 38px; border-radius: 11px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; box-shadow: var(--shadow-1); }
.ax-box input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.ax-box > svg { position: absolute; left: 12px; top: 50%; width: 17px; height: 17px; transform: translateY(-50%); color: var(--muted); pointer-events: none; fill: none; stroke: currentColor; stroke-width: 2; }
.ax-box .kbd { position: absolute; right: 10px; top: 50%; transform: translateY(-50%); font: 11px var(--mono); color: var(--muted); border: 1px solid var(--line-2); border-radius: 5px; padding: 1px 6px; }
.ax-sugg { position: absolute; left: 0; right: 0; top: calc(100% + 6px); background: var(--surface); border: 1px solid var(--line-2); border-radius: 11px; box-shadow: var(--shadow-2); padding: 6px; display: none; z-index: 9; max-height: 70vh; overflow: auto; }
.ax-sugg.on { display: block; }
.ax-sugg .it { display: flex; gap: 10px; align-items: baseline; padding: 7px 9px; border-radius: 7px; cursor: pointer; }
.ax-sugg .it.sel, .ax-sugg .it:hover { background: var(--accent-soft); }
.ax-sugg .it .k { font-size: 10.5px; color: var(--muted); width: 64px; flex: none; text-transform: uppercase; letter-spacing: .05em; }
.ax-sugg .it .t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-sugg .it .m { margin-left: auto; color: var(--muted); font-size: 12px; white-space: nowrap; }
.ax-nav { display: flex; gap: 2px; margin-top: 8px; overflow-x: auto; scrollbar-width: none; align-items: center; }
.ax-nav a { padding: 5px 11px; border-radius: 8px; color: var(--text-2); font-size: 13px; white-space: nowrap; }
.ax-nav a:hover { background: var(--surface-2); text-decoration: none; }
.ax-nav a.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.ax-nav .sp { flex: 1; }
.ax-nav .live { display: inline-flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; padding: 5px 4px; white-space: nowrap; }
.ax-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--good); animation: axpulse 2s infinite; }
@keyframes axpulse { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--good) 55%, transparent); } 70% { box-shadow: 0 0 0 7px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
.ax-main { padding: 8px 0 80px; min-height: 60vh; }
.ax-und { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin: 4px 0 12px; font-size: 12.5px; color: var(--muted); }
.ax-chip { display: inline-flex; align-items: center; gap: 5px; padding: 2px 9px; border-radius: 999px; background: var(--surface); border: 1px solid var(--line-2); color: var(--text-2); font-size: 12.5px; white-space: nowrap; cursor: pointer; line-height: 1.6; }
.ax-chip:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.ax-chip.f { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.ax-chip.t { background: color-mix(in srgb, var(--s7) 12%, var(--surface)); border-color: transparent; color: color-mix(in srgb, var(--s7) 80%, var(--text)); }
.ax-chip.d { background: color-mix(in srgb, var(--s3) 13%, var(--surface)); border-color: transparent; color: color-mix(in srgb, var(--s3) 75%, var(--text)); }
.ax-chip.c { font-family: var(--mono); font-size: 11.5px; }
.ax-chip .x { opacity: .6; font-size: 13px; line-height: 1; }
.ax-chip .from { color: var(--muted); font-family: var(--mono); font-size: 11px; }
.ax-grid { display: grid; grid-template-columns: 220px minmax(0, 1fr) 290px; gap: 26px; align-items: start; }
.ax-grid.two { grid-template-columns: minmax(0, 1fr) 320px; }
@media (max-width: 1120px) { .ax-grid { grid-template-columns: 200px minmax(0, 1fr); } .ax-grid > .ax-right { display: none; } }
@media (max-width: 780px) { .ax-grid, .ax-grid.two { grid-template-columns: minmax(0, 1fr); } .ax-grid > .ax-left { display: none; } .ax-brand small { display: none; } }
.ax-left { position: sticky; top: 108px; max-height: calc(100vh - 120px); overflow: auto; scrollbar-width: thin; padding-right: 4px; }
.ax-facet { margin-bottom: 18px; }
.ax-facet h4 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; }
.ax-facet .r { display: flex; align-items: center; gap: 8px; padding: 3px 6px; margin: 0 -6px; border-radius: 6px; cursor: pointer; font-size: 13px; color: var(--text-2); }
.ax-facet .r:hover { background: var(--surface-2); color: var(--text); }
.ax-facet .r .n { margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; }
.ax-facet .r .l { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-years { display: flex; align-items: flex-end; gap: 1px; height: 54px; margin: 4px 0 2px; }
.ax-years div { flex: 1; background: color-mix(in srgb, var(--accent) 55%, transparent); border-radius: 2px 2px 0 0; min-height: 2px; cursor: pointer; }
.ax-years div:hover { background: var(--accent); }
.ax-yl { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--muted); }
.ax-head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 4px; }
.ax-head .t { font-size: 13px; color: var(--muted); }
.ax-head .t b { color: var(--text); font-weight: 600; }
.ax-head .sp { flex: 1; }
.ax-seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 8px; overflow: hidden; background: var(--surface); }
.ax-seg button { font: 12.5px var(--sans); border: 0; background: none; color: var(--text-2); padding: 4px 10px; cursor: pointer; }
.ax-seg button + button { border-left: 1px solid var(--line); }
.ax-seg button.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.ax-dym { font-size: 14px; margin: 2px 0 10px; color: var(--text-2); }
.ax-dym a { color: var(--accent); font-weight: 600; font-style: italic; }
.ax-list { list-style: none; margin: 0; padding: 0; }
.ax-p { padding: 13px 0; border-bottom: 1px solid var(--line); }
.ax-p:last-child { border-bottom: 0; }
.ax-p .ttl { font-size: 15.5px; font-weight: 580; line-height: 1.35; letter-spacing: -.006em; }
.ax-p .ttl a { color: var(--text); }
.ax-p .au { font-size: 13px; color: var(--text-2); margin-top: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-p .au a:hover { color: var(--accent); }
.ax-p .meta { display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12.5px; color: var(--muted); margin-top: 4px; align-items: center; }
.ax-p .meta a:hover { color: var(--accent); }
.ax-p .abs { font-size: 13.5px; color: var(--text-2); margin-top: 6px; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
.ax-p .tags { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; }
.ax-p.compact { padding: 8px 0; }
.ax-p.compact .ttl { font-size: 14px; font-weight: 560; }
.ax-p .why { font-size: 11.5px; color: var(--accent); margin-top: 3px; }
.ax-badge { font-size: 10.5px; font-weight: 650; padding: 0 6px; border-radius: 5px; background: var(--surface-3); color: var(--text-2); text-transform: uppercase; letter-spacing: .04em; line-height: 1.7; }
.ax-badge.code { background: var(--good-soft); color: var(--good-ink); }
.ax-badge.survey { background: var(--accent-soft); color: var(--accent); }
.ax-badge.benchmark, .ax-badge.dataset { background: var(--warn-soft); color: color-mix(in srgb, var(--warn) 50%, var(--text)); }
.ax-badge.tool { background: var(--serious-soft); color: var(--s2); }
.ax-cites { font-variant-numeric: tabular-nums; color: var(--text-2); }
.ax-cites b { color: var(--text); font-weight: 650; }
.ax-sec { display: flex; align-items: baseline; justify-content: space-between; margin: 24px 0 6px; gap: 10px; }
.ax-sec h3 { margin: 0; font-size: 12.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
.ax-sec a, .ax-sec .a { font-size: 13px; color: var(--accent); cursor: pointer; }
.ax-sec .note { font-size: 12px; color: var(--muted); }
.ax-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; box-shadow: var(--shadow-1); }
.ax-card h4 { margin: 0 0 8px; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; display: flex; justify-content: space-between; gap: 8px; }
.ax-card h4 a { text-transform: none; letter-spacing: 0; color: var(--accent); font-weight: 500; }
.ax-ans { border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--line)); background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 6%, var(--surface)), var(--surface)); border-radius: var(--r); padding: 14px 16px 12px; margin: 10px 0 14px; }
.ax-ans .ey { font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--accent); font-weight: 680; display: flex; gap: 8px; align-items: center; }
.ax-ans .ey .nm { color: var(--muted); font-weight: 500; text-transform: none; letter-spacing: 0; margin-left: auto; }
.ax-ans .big { font-size: 17.5px; font-weight: 640; line-height: 1.38; letter-spacing: -.008em; margin-top: 5px; }
.ax-ans ol { margin: 8px 0 0; padding: 0; list-style: none; counter-reset: r; }
.ax-ans ol li { counter-increment: r; display: grid; grid-template-columns: 22px 1fr auto; gap: 8px; align-items: center; padding: 5px 0; border-top: 1px solid var(--line); font-size: 13.5px; }
.ax-ans ol li:first-child { border-top: 0; }
.ax-ans ol li:before { content: counter(r); color: var(--muted); font-size: 12px; text-align: right; }
.ax-ans ol li .n { color: var(--muted); font-size: 12.5px; font-variant-numeric: tabular-nums; white-space: nowrap; }
.ax-ans ol li .who { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-ans .cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 16px; margin-top: 8px; }
.ax-ans .cols h5 { margin: 0 0 2px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); }
.ax-sum { font-size: 14.5px; line-height: 1.6; }
.ax-sum p { margin: 0 0 8px; }
.ax-sum ul { margin: 4px 0 8px; padding-left: 20px; }
.ax-sum li { margin: 4px 0; }
.ax-sum sup a { color: var(--accent); font-size: 10.5px; font-weight: 650; padding: 0 1px; }
.ax-src { font-size: 12.5px; color: var(--muted); display: grid; grid-template-columns: 24px 1fr; gap: 2px 6px; margin-top: 8px; }
.ax-src a { color: var(--text-2); }
.ax-btn { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); cursor: pointer; font-size: 13px; white-space: nowrap; }
.ax-btn:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.ax-btn.pri { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); font-weight: 600; }
.ax-btn.pri:hover { background: var(--accent-2); color: var(--accent-ink); }
.ax-btn.ghost { border-color: transparent; background: none; }
.ax-btn[disabled] { opacity: .55; cursor: default; }
.ax-btn.on { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.ax-empty { color: var(--muted); padding: 26px 0; text-align: center; }
.ax-skel { height: 14px; border-radius: 6px; background: linear-gradient(90deg, var(--surface-2), var(--surface-3), var(--surface-2)); background-size: 200% 100%; animation: axsk 1.2s infinite; margin: 10px 0; }
@keyframes axsk { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
.ax-err { color: var(--crit-ink); background: var(--crit-soft); padding: 10px 12px; border-radius: 8px; font-size: 13px; }
.ax-hero { text-align: center; padding: 46px 0 22px; }
.ax-hero h1 { margin: 0; font-size: 38px; letter-spacing: -.03em; line-height: 1.08; font-weight: 760; }
.ax-hero h1 span { background: linear-gradient(90deg, var(--accent), var(--s7)); -webkit-background-clip: text; background-clip: text; color: transparent; }
.ax-hero p { color: var(--text-2); font-size: 16px; margin: 12px auto 0; max-width: 700px; }
.ax-stats { display: flex; justify-content: center; flex-wrap: wrap; gap: 8px 26px; margin: 18px 0 6px; color: var(--muted); font-size: 13px; }
.ax-stats b { color: var(--text); font-size: 17px; font-weight: 700; font-variant-numeric: tabular-nums; margin-right: 4px; }
.ax-try { display: flex; flex-wrap: wrap; justify-content: center; gap: 7px; margin: 16px auto 4px; max-width: 920px; }
.ax-try a { padding: 5px 12px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); font-size: 13px; color: var(--text-2); }
.ax-try a:hover { border-color: var(--accent); color: var(--accent); text-decoration: none; }
.ax-cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 22px; }
.ax-rise { display: grid; grid-template-columns: 1fr auto; gap: 2px 10px; font-size: 13.5px; align-items: baseline; }
.ax-rise .g { font-variant-numeric: tabular-nums; font-weight: 650; color: var(--good-ink); font-size: 12.5px; }
.ax-rise .g.down { color: var(--crit-ink); }
.ax-rise a { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-rise .s { grid-column: 1 / -1; font-size: 12px; color: var(--muted); margin: -2px 0 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-pg { display: flex; gap: 8px; align-items: center; justify-content: center; margin: 16px 0; font-size: 13px; color: var(--muted); }
.ax-ph { padding: 4px 0 8px; }
.ax-ph .ey { font-size: 12px; color: var(--muted); display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
.ax-ph h1 { margin: 6px 0 6px; font-size: 27px; line-height: 1.2; letter-spacing: -.02em; font-weight: 720; }
.ax-ph .au { font-size: 14.5px; color: var(--text-2); }
.ax-ph .au a { color: var(--text); }
.ax-ph .au a:hover { color: var(--accent); }
.ax-ph .au .more { color: var(--accent); cursor: pointer; }
.ax-ph .abs { font-size: 15px; line-height: 1.62; margin: 14px 0; color: var(--text); max-width: 820px; }
.ax-ph .acts { display: flex; flex-wrap: wrap; gap: 8px; margin: 10px 0; }
.ax-ph .tags { display: flex; flex-wrap: wrap; gap: 6px; margin: 8px 0; }
.ax-kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(92px, 1fr)); gap: 10px; margin: 12px 0; }
.ax-kpi { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 10px 12px; }
.ax-kpi .v { font-size: 22px; font-weight: 720; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.ax-kpi .l { font-size: 11.5px; color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }
.ax-tldr { border-left: 3px solid var(--accent); padding: 8px 14px; background: var(--accent-soft); border-radius: 0 8px 8px 0; margin: 10px 0; font-size: 14.5px; max-width: 820px; }
.ax-tldr b { color: var(--accent); }
.ax-tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); margin: 18px 0 4px; overflow-x: auto; scrollbar-width: none; }
.ax-tabs button { border: 0; background: none; padding: 8px 12px; color: var(--text-2); cursor: pointer; border-bottom: 2px solid transparent; white-space: nowrap; font-size: 13.5px; }
.ax-tabs button.on { color: var(--accent); border-bottom-color: var(--accent); font-weight: 600; }
.ax-tabs button .n { color: var(--muted); font-size: 12px; margin-left: 4px; }
.ax-people { display: grid; grid-template-columns: 1fr auto; gap: 3px 10px; font-size: 13.5px; }
.ax-people a { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-people .n { color: var(--muted); font-size: 12.5px; font-variant-numeric: tabular-nums; text-align: right; }
.ax-chart { width: 100%; display: block; overflow: visible; }
.ax-chart text { fill: var(--muted); font: 10.5px var(--sans); }
.ax-chart .grid { stroke: var(--grid); stroke-width: 1; }
.ax-chart .ln { fill: none; stroke-width: 2.2; stroke-linejoin: round; stroke-linecap: round; }
.ax-chart .ar { opacity: .12; }
.ax-chart .hov { stroke: var(--axis); stroke-dasharray: 3 3; }
.ax-legend { display: flex; flex-wrap: wrap; gap: 6px 14px; font-size: 12.5px; color: var(--text-2); margin: 6px 0; }
.ax-legend i { display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 6px; vertical-align: -1px; }
.ax-tip { position: fixed; z-index: 30; pointer-events: none; background: var(--surface); border: 1px solid var(--line-2); border-radius: 8px; box-shadow: var(--shadow-2); padding: 7px 10px; font-size: 12.5px; display: none; max-width: 320px; }
.ax-tip b { display: block; margin-bottom: 2px; }
.ax-atlas { position: relative; height: calc(100vh - 170px); min-height: 520px; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); overflow: hidden; }
.ax-atlas canvas { width: 100%; height: 100%; display: block; cursor: grab; }
.ax-atlas .side { position: absolute; top: 10px; left: 10px; width: 300px; max-height: calc(100% - 20px); overflow: auto; background: color-mix(in srgb, var(--surface) 92%, transparent); backdrop-filter: blur(8px); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; box-shadow: var(--shadow-2); }
.ax-atlas .side input { width: 100%; padding: 6px 9px; border-radius: 7px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); margin-bottom: 8px; }
.ax-atlas .hint { position: absolute; right: 12px; bottom: 10px; font-size: 12px; color: var(--muted); }
.ax-studio { display: grid; grid-template-columns: 380px minmax(0, 1fr); gap: 24px; align-items: start; }
@media (max-width: 960px) { .ax-studio { grid-template-columns: minmax(0, 1fr); } }
.ax-form { position: sticky; top: 108px; max-height: calc(100vh - 120px); overflow: auto; scrollbar-width: thin; padding-right: 6px; }
.ax-f { margin-bottom: 14px; }
.ax-f > label { display: block; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; margin-bottom: 5px; }
.ax-f > label small { text-transform: none; letter-spacing: 0; font-weight: 400; }
.ax-f input[type=text], .ax-f textarea, .ax-f select { width: 100%; padding: 7px 10px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); font: 13.5px var(--sans); outline: none; }
.ax-f input[type=text]:focus, .ax-f textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.ax-f input[type=range] { width: 100%; accent-color: var(--accent); }
.ax-f .row { display: flex; gap: 8px; align-items: center; }
.ax-f .row > * { flex: 1; }
.ax-f .val { font-variant-numeric: tabular-nums; color: var(--text); font-weight: 600; flex: none; min-width: 36px; text-align: right; font-size: 13px; }
.ax-tagsin { display: flex; flex-wrap: wrap; gap: 5px; padding: 5px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); min-height: 36px; align-items: center; }
.ax-tagsin:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.ax-tagsin input { border: 0 !important; box-shadow: none !important; flex: 1; min-width: 120px; padding: 3px 4px !important; background: transparent !important; }
.ax-tagsin .ax-chip { cursor: default; }
.ax-tagsin .ax-chip .x { cursor: pointer; }
.ax-check { display: flex; flex-wrap: wrap; gap: 6px; }
.ax-check label { display: inline-flex; gap: 5px; align-items: center; font-size: 13px; padding: 3px 9px; border-radius: 999px; border: 1px solid var(--line-2); cursor: pointer; background: var(--surface); }
.ax-check label.on { background: var(--accent-soft); color: var(--accent); border-color: transparent; }
.ax-check input { display: none; }
.ax-news { max-width: 760px; }
.ax-news h1 { font-size: 30px; letter-spacing: -.02em; margin: 6px 0 4px; line-height: 1.15; }
.ax-news .tag { color: var(--muted); font-size: 15px; margin-bottom: 18px; }
.ax-news .item { padding: 16px 0 18px; border-top: 1px solid var(--line); }
.ax-news .item h2 { font-size: 19px; letter-spacing: -.01em; margin: 0 0 8px; line-height: 1.3; }
.ax-news .item h2 .n { color: var(--muted); font-weight: 500; margin-right: 6px; }
.ax-news .item p { font-size: 15px; line-height: 1.66; margin: 0 0 8px; }
.ax-news .links { display: flex; flex-wrap: wrap; gap: 6px 16px; font-size: 13.5px; }
.ax-news .links a { color: var(--accent); }
.ax-news .extra li { margin: 6px 0; font-size: 14px; }
.ax-news .open { font-size: 16px; line-height: 1.7; color: var(--text); margin: 0 0 18px; }
.ax-news .open a, .ax-news .toc a { color: var(--accent); }
.ax-news .toc { counter-reset: t; list-style: none; padding: 12px 14px; margin: 0 0 18px; border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); }
.ax-news .toc li { counter-increment: t; display: grid; grid-template-columns: 26px 1fr; gap: 2px 6px; padding: 5px 0; font-size: 14px; line-height: 1.45; }
.ax-news .toc li::before { content: counter(t) "."; color: var(--muted); font-weight: 600; }
.ax-news .toc li span { grid-column: 2; color: var(--text-2); font-size: 13px; }
.ax-news .who { font-size: 12.5px; color: var(--muted); margin: -2px 0 10px; }
.ax-news .brief { font-size: 16.5px; line-height: 1.6; margin: 0 0 14px; padding: 10px 14px; border-left: 3px solid var(--accent); background: color-mix(in srgb, var(--accent) 7%, transparent); border-radius: 0 8px 8px 0; }
.ax-news .lab { display: block; font-size: 11px; font-weight: 700; letter-spacing: .07em; text-transform: uppercase; color: var(--muted); margin: 14px 0 3px; }
.ax-news .lab + p { margin-top: 0; }
.ax-news .fig { margin: 12px 0 14px; }
.ax-news .fig .imgs { display: flex; gap: 8px; flex-wrap: wrap; justify-content: center; background: #fff; border: 1px solid var(--line); border-radius: 10px; padding: 10px; }
.ax-news .fig img { max-width: 100%; max-height: 380px; object-fit: contain; flex: 1 1 0; min-width: 0; }
.ax-news .fig .ph { flex: 1 1 0; min-height: 160px; border-radius: 6px; background: linear-gradient(90deg, #f1f2f4, #e7e9ec, #f1f2f4); background-size: 200% 100%; animation: ax-sh 1.4s infinite; }
.ax-news .fig figcaption { font-size: 13.5px; line-height: 1.5; color: var(--text-2); margin-top: 6px; }
.ax-news .fig figcaption b { color: var(--text); }
.ax-news .fig details { font-size: 12.5px; color: var(--muted); margin-top: 4px; }
.ax-news .fig summary { cursor: pointer; }
.ax-news .tbl { overflow-x: auto; margin: 6px 0 4px; border: 1px solid var(--line); border-radius: 8px; }
.ax-news .tbl table { border-collapse: collapse; font-size: 12.5px; width: 100%; }
.ax-news .tbl td { padding: 4px 8px; border-top: 1px solid var(--line); white-space: nowrap; font-variant-numeric: tabular-nums; }
.ax-news .tbl tr:first-child td { border-top: 0; font-weight: 600; background: var(--surface); }
.ax-news .terms { display: grid; grid-template-columns: max-content 1fr; gap: 4px 12px; font-size: 13.5px; margin: 4px 0 10px; }
.ax-news .terms dt { font-weight: 600; }
.ax-news .terms dd { margin: 0; color: var(--text-2); }
.ax-news .note { font-size: 12.5px; color: var(--muted); margin: 4px 0 8px; }
@keyframes ax-sh { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
.ax-prev .ax-p .found { font-size: 11.5px; color: var(--muted); margin-top: 4px; }
.ax-prev .ax-p .found span { background: var(--surface-3); border-radius: 5px; padding: 0 5px; margin-right: 4px; }
.ax-prog { height: 4px; border-radius: 2px; background: var(--surface-3); overflow: hidden; margin: 8px 0; }
.ax-prog div { height: 100%; background: var(--accent); width: 30%; animation: axprog 1.4s infinite ease-in-out; }
@keyframes axprog { 0% { margin-left: -30%; } 100% { margin-left: 100%; } }
.ax-sql { position: fixed; right: 18px; bottom: 16px; z-index: 20; }
.ax-sql button { font: 12px var(--mono); padding: 6px 11px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); cursor: pointer; box-shadow: var(--shadow-2); }
.ax-sql button b { color: var(--accent); }
.ax-sqlp { position: fixed; right: 18px; bottom: 56px; width: min(760px, calc(100vw - 36px)); max-height: 70vh; overflow: auto; z-index: 21; background: var(--surface); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: var(--shadow-3); padding: 12px 14px; display: none; }
.ax-sqlp.on { display: block; }
.ax-sqlp .q { border-top: 1px solid var(--line); padding: 8px 0; }
.ax-sqlp .w { display: flex; justify-content: space-between; gap: 10px; font-size: 12.5px; color: var(--text-2); font-weight: 600; }
.ax-sqlp .w span { color: var(--muted); font-weight: 400; white-space: nowrap; font-variant-numeric: tabular-nums; }
.ax-sqlp pre { margin: 5px 0 0; font: 11.5px/1.5 var(--mono); white-space: pre-wrap; word-break: break-word; background: var(--code); padding: 8px 10px; border-radius: 7px; color: var(--text); }
.ax-sqlp .bad { color: var(--crit-ink); font-size: 12px; }
.ax-foot { color: var(--muted); font-size: 12px; text-align: center; padding: 30px 0 60px; border-top: 1px solid var(--line); margin-top: 30px; }
.ax-foot a { color: var(--accent); }
.ax-about { max-width: 780px; font-size: 14.5px; line-height: 1.65; }
.ax-about h2 { font-size: 18px; margin: 22px 0 6px; }
.ax-about code { font: 12.5px var(--mono); background: var(--code); padding: 1px 5px; border-radius: 4px; }
.ax-about pre { font: 12px/1.5 var(--mono); background: var(--code); padding: 10px 12px; border-radius: 8px; overflow: auto; }
`;

const ICON = `<svg viewBox="0 0 24 24"><path d="M3 5.5c2.6-1 5.4-1 8 .6v13c-2.6-1.6-5.4-1.6-8-.6z"/><path d="M21 5.5c-2.6-1-5.4-1-8 .6v13c2.6-1.6 5.4-1.6 8-.6z"/><path d="M17.5 2v3M16 3.5h3"/></svg>`;
const SEARCH = `<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>`;
const PALETTE = ["var(--s1)", "var(--s2)", "var(--s3)", "var(--s4)", "var(--s5)", "var(--s7)", "var(--s8)", "var(--s6)"];
const FIELD_COLORS = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#6250d6", "#e34948", "#008300", "#0aa5b5", "#a35be0", "#7f8c1d", "#c2410c", "#2563eb", "#db2777", "#059669", "#9333ea"];

// ---------------------------------------------------------------- helpers

function h(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "style" && typeof v === "object") Object.assign(e.style, v);
    else if (k === "html") e.innerHTML = v;
    else if (k.startsWith("on") && typeof v === "function") e.addEventListener(k.slice(2), v);
    else if (k === "value") e.value = v;
    else if (k === "checked") e.checked = !!v;
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
const short = (n) => { n = Number(n) || 0; return n >= 1e6 ? (n / 1e6).toFixed(n >= 1e7 ? 0 : 1) + "M" : n >= 1e4 ? Math.round(n / 1e3) + "k" : n >= 1e3 ? (n / 1e3).toFixed(1) + "k" : String(n); };
const day = (t) => (t ? String(t).slice(0, 10) : "");
function ago(t) {
  if (!t) return "";
  const s = (Date.now() - Date.parse(t)) / 1000;
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 45) return `${Math.round(s / 86400)} days ago`;
  return day(t);
}
function esc(s) { return String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }
function skel(n = 5) { return Array.from({ length: n }, (_, i) => h("div", { class: "ax-skel", style: { width: `${60 + ((i * 37) % 40)}%` } })); }
function userId() {
  let u = null;
  try { u = localStorage.getItem("ax-user"); } catch {}
  if (!u) {
    u = "r" + Array.from(crypto.getRandomValues(new Uint8Array(9)), (b) => b.toString(16).padStart(2, "0")).join("");
    try { localStorage.setItem("ax-user", u); } catch {}
  }
  return u;
}
const CAT_NAMES = {};

// --------------------------------------------------------------- charts

let tip;
function showTip(ev, html) {
  if (!tip) { tip = h("div", { class: "ax-tip" }); document.body.append(tip); }
  tip.innerHTML = html;
  tip.style.display = "block";
  const x = Math.min(ev.clientX + 14, innerWidth - tip.offsetWidth - 10);
  const y = Math.min(ev.clientY + 14, innerHeight - tip.offsetHeight - 10);
  tip.style.left = x + "px"; tip.style.top = y + "px";
}
function hideTip() { if (tip) tip.style.display = "none"; }

// Lines over years: series = [{name, color, points: [[x, y]]}], y formatted by fy.
function lineChart(series, { height = 190, fy = short, ylabel = "", area = false } = {}) {
  const W = 640, H = height, L = 44, R = 12, T = 10, B = 24;
  const xs = series.flatMap((s) => s.points.map((p) => p[0]));
  const ys = series.flatMap((s) => s.points.map((p) => p[1]));
  if (!xs.length) return h("div", { class: "ax-empty" }, "No data");
  const x0 = Math.min(...xs), x1 = Math.max(...xs), ymax = Math.max(1e-9, ...ys) * 1.08;
  const X = (x) => L + ((x - x0) / Math.max(1, x1 - x0)) * (W - L - R);
  const Y = (y) => T + (1 - y / ymax) * (H - T - B);
  const g = svg("svg", { class: "ax-chart", viewBox: `0 0 ${W} ${H}`, preserveAspectRatio: "none", style: `height:${H}px` });
  for (let i = 0; i <= 4; i++) {
    const v = (ymax / 1.08) * (i / 4), y = Y(v);
    g.append(svg("line", { class: "grid", x1: L, x2: W - R, y1: y, y2: y }), svg("text", { x: L - 6, y: y + 3.5, "text-anchor": "end" }, fy(v)));
  }
  const step = Math.max(1, Math.ceil((x1 - x0) / 8));
  for (let x = x0; x <= x1; x += step) g.append(svg("text", { x: X(x), y: H - 6, "text-anchor": "middle" }, String(x)));
  if (ylabel) g.append(svg("text", { x: L, y: T - 1, "text-anchor": "start" }, ylabel));
  series.forEach((s, i) => {
    const c = s.color || PALETTE[i % PALETTE.length];
    const pts = s.points.slice().sort((a, b) => a[0] - b[0]);
    const d = pts.map((p, j) => `${j ? "L" : "M"}${X(p[0]).toFixed(1)},${Y(p[1]).toFixed(1)}`).join("");
    if (area || series.length === 1) g.append(svg("path", { class: "ar", d: `${d}L${X(pts[pts.length - 1][0])},${Y(0)}L${X(pts[0][0])},${Y(0)}Z`, fill: c }));
    g.append(svg("path", { class: "ln", d, stroke: c }));
  });
  const hov = svg("line", { class: "hov", y1: T, y2: H - B, x1: -10, x2: -10 });
  g.append(hov);
  const rect = svg("rect", { x: L, y: T, width: W - L - R, height: H - T - B, fill: "transparent" });
  rect.addEventListener("mousemove", (ev) => {
    const b = g.getBoundingClientRect();
    const px = ((ev.clientX - b.left) / b.width) * W;
    const xv = Math.round(x0 + ((px - L) / (W - L - R)) * (x1 - x0));
    hov.setAttribute("x1", X(xv)); hov.setAttribute("x2", X(xv));
    const rows = series.map((s, i) => { const p = s.points.find((q) => q[0] === xv); return p ? `<div><i style="display:inline-block;width:8px;height:8px;border-radius:2px;background:${s.color || PALETTE[i % PALETTE.length]};margin-right:6px"></i>${esc(s.name)}: <b style="display:inline">${fy(p[1])}</b>${s.extra ? ` <span style="color:var(--muted)">${s.extra(p)}</span>` : ""}</div>` : ""; }).join("");
    showTip(ev, `<b>${xv}</b>${rows}`);
  });
  rect.addEventListener("mouseleave", () => { hideTip(); hov.setAttribute("x1", -10); hov.setAttribute("x2", -10); });
  g.append(rect);
  const wrap = h("div", {}, g);
  if (series.length > 1) wrap.prepend(h("div", { class: "ax-legend" }, series.map((s, i) => h("span", {}, h("i", { style: { background: s.color || PALETTE[i % PALETTE.length] } }), s.name))));
  return wrap;
}

// Bars for a year histogram: [{year, n}].
function yearBars(years, onPick) {
  const ys = years.filter((y) => y.value >= 1991 || y.year >= 1991);
  if (!ys.length) return null;
  const max = Math.max(...ys.map((y) => y.n));
  const first = ys[0].value ?? ys[0].year, last = ys[ys.length - 1].value ?? ys[ys.length - 1].year;
  return h("div", {},
    h("div", { class: "ax-years" }, ys.map((y) => {
      const yr = y.value ?? y.year;
      const d = h("div", { style: { height: `${Math.max(3, (y.n / max) * 100)}%` }, onclick: () => onPick && onPick(yr) });
      d.addEventListener("mousemove", (ev) => showTip(ev, `<b>${yr}</b>${fmt(y.n)} papers`));
      d.addEventListener("mouseleave", hideTip);
      return d;
    })),
    h("div", { class: "ax-yl" }, h("span", {}, first), h("span", {}, last)));
}

// --------------------------------------------------------------- pieces

function authorsLine(app, r, max = 6) {
  const people = r.people || [];
  const names = String(r.authors || "").split(", ");
  const out = [];
  names.slice(0, max).forEach((n, i) => {
    if (i) out.push(", ");
    out.push(people[i] ? h("a", { href: app.href("author", people[i]) }, n) : n);
  });
  const total = r.nauthors || names.length;
  if (total > max) out.push(h("span", { style: { color: "var(--muted)" } }, ` + ${total - max} more`));
  return out;
}

function catChip(app, c) { return h("a", { class: "ax-chip c", href: app.href("category", c), title: CAT_NAMES[c] || "" }, c); }
function conceptChip(app, id, name, kind) { return h("a", { class: `ax-chip ${kind === "domain" ? "d" : "t"}`, href: app.href("concept", id) }, name || id); }

function paperItem(app, r, { compact = false, why = null, extra = null } = {}) {
  const kinds = (r.kinds || []).filter((k) => k !== "theory");
  const cites = r.cites || 0;
  const meta = h("div", { class: "meta" },
    h("span", {}, r.year || day(r.created).slice(0, 4)),
    r.pcat ? catChip(app, r.pcat) : null,
    cites ? h("span", { class: "ax-cites", title: "Citations (Crossref)" }, h("b", {}, fmt(cites)), " citations") : h("span", {}, ago(r.created)),
    r.venue ? h("a", { href: app.href("venue", r.venue) }, r.venue + (r.venue_year ? ` ${r.venue_year}` : "")) : null,
    r.code ? h("a", { class: "ax-badge code", href: r.code, target: "_blank", rel: "noopener" }, "code") : null,
    kinds.map((k) => h("span", { class: `ax-badge ${k}` }, k)),
    h("a", { href: `https://arxiv.org/abs/${r.id}`, target: "_blank", rel: "noopener", style: { fontFamily: "var(--mono)", fontSize: "11.5px" } }, r.id),
    r.score != null && extra === "score" ? h("span", { title: "Score" }, Number(r.score).toFixed(3)) : null,
    r.together ? h("span", { title: "Works that cite both" }, `cited together ${fmt(r.together)}×`) : null);
  const title = h("div", { class: "ttl" }, h("a", { href: app.href("paper", r.id), html: r.title_hl || esc(r.title) }));
  const kids = [title, h("div", { class: "au" }, authorsLine(app, r, compact ? 3 : 6)), meta];
  if (!compact && (r.abstract_hl || r.abstract)) kids.push(h("div", { class: "abs", html: r.abstract_hl || esc(r.abstract) }));
  if (!compact) {
    const t = (r.techniques || []).slice(0, 4), d = (r.domains || []).slice(0, 3);
    if (t.length || d.length) kids.push(h("div", { class: "tags" }, t.map((x) => conceptChip(app, x, app.concept(x), "technique")), d.map((x) => conceptChip(app, x, app.concept(x), "domain"))));
  }
  if (why) kids.push(h("div", { class: "why" }, why));
  return h("li", { class: `ax-p${compact ? " compact" : ""}` }, kids);
}

function paperList(app, rows, o) {
  if (!rows || !rows.length) return h("div", { class: "ax-empty" }, "Nothing here yet.");
  return h("ul", { class: "ax-list" }, rows.map((r) => paperItem(app, r, o)));
}

function peopleList(app, rows, { n = "n", label = "papers" } = {}) {
  if (!rows || !rows.length) return h("div", { class: "ax-empty" }, "No one yet.");
  return h("div", { class: "ax-people" }, rows.map((p) => [h("a", { href: app.href("author", p.id) }, p.name || p.id), h("span", { class: "n" }, `${fmt(p[n])} ${label}`)]));
}

function conceptList(app, rows, kind) {
  if (!rows || !rows.length) return null;
  return h("div", { class: "ax-people" }, rows.map((c) => [h("a", { href: app.href("concept", c.id) }, c.name || c.id), h("span", { class: "n" }, fmt(c.n))]));
}

function sec(title, right) { return h("div", { class: "ax-sec" }, h("h3", {}, title), right || null); }

function sharesChart(years, { label = "papers in every 10,000" } = {}) {
  const pts = (years || []).filter((y) => y.year >= 1993).map((y) => [y.year, y.per10k]);
  const cnt = (years || []).filter((y) => y.year >= 1993);
  return lineChart([{ name: "share of arXiv", points: pts, extra: (p) => { const c = cnt.find((y) => y.year === p[0]); return c ? `${fmt(c.n)} papers` : ""; } }], { fy: (v) => (v >= 100 ? Math.round(v) : v.toFixed(v < 10 ? 1 : 0)), ylabel: label });
}

// Markdown-ish text from a model: paragraphs, bullets, [n] citations linked.
function richText(app, text, sources) {
  const wrap = h("div", { class: "ax-sum" });
  const cite = (s) => esc(s).replace(/\*\*(.+?)\*\*/g, "<b>$1</b>").replace(/\[(\d+)\]/g, (m, n) => { const src = sources && sources[Number(n) - 1]; return src ? `<sup><a href="${app.href("paper", src.id)}" title="${esc(src.title)}">${n}</a></sup>` : m; });
  let ul = null;
  for (const line of String(text || "").split(/\n+/)) {
    const t = line.trim();
    if (!t) continue;
    if (/^[-*•] /.test(t)) { if (!ul) { ul = h("ul"); wrap.append(ul); } ul.append(h("li", { html: cite(t.replace(/^[-*•] /, "")) })); }
    else { ul = null; wrap.append(h("p", { html: cite(t.replace(/^#+\s*/, "")) })); }
  }
  return wrap;
}

// --------------------------------------------------------------- the app

function render(el, ctx) {
  if (!document.getElementById("ax-css")) document.head.append(h("style", { id: "ax-css" }, CSS));
  const app = {
    ctx,
    queries: [],
    took: 0,
    concepts: {},
    concept: (s) => app.concepts[s] || s,
    api: async (path, opts) => {
      const j = await ctx.api(path, opts);
      if (j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...parts),
    go: (...parts) => { location.hash = ctx.href(...parts); },
    user: userId(),
  };
  const root = h("div", { class: "ax" });
  fill(el, root);
  const input = h("input", { type: "search", placeholder: "Search 3 million papers, or ask: “who writes most about diffusion models since 2022”", autocomplete: "off", spellcheck: "false", "aria-label": "Search arXiv" });
  const sugg = h("div", { class: "ax-sugg" });
  const nav = h("nav", { class: "ax-nav" });
  const live = h("span", { class: "live" });
  const box = h("div", { class: "ax-box" }, input, h("span", { class: "kbd" }, "/"), sugg);
  box.insertAdjacentHTML("afterbegin", SEARCH);
  const top = h("div", { class: "ax-top" }, h("div", { class: "ax-bar" }, h("a", { class: "ax-brand", href: app.href(""), html: `${ICON}<span>arXiv Explorer <small>on ChloeDB</small></span>` }), box), nav);
  const main = h("main", { class: "ax-main" });
  const sqlBtn = h("button", { onclick: () => sqlPanel.classList.toggle("on"), title: "The SQL behind this page" });
  const sqlPanel = h("div", { class: "ax-sqlp" });
  root.append(top, main, h("div", { class: "ax-sql" }, sqlBtn), sqlPanel,
    h("div", { class: "ax-foot" }, "Paper metadata from arXiv (its OAI-PMH interface and the Kaggle metadata snapshot, CC0), citations from Crossref. Thank you to arXiv for use of its open access interoperability. Not affiliated with arXiv or Cornell University. ",
      h("a", { href: app.href("about") }, "How this works")));

  function drawSql() {
    const ms = app.queries.reduce((a, q) => a + (q.ms || 0), 0);
    fill(sqlBtn, "SQL · ", h("b", {}, String(app.queries.length)), ` ${app.queries.length === 1 ? "query" : "queries"} · ${Math.round(app.took || ms)} ms`);
    fill(sqlPanel, h("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "6px" } },
      h("b", {}, "The SQL behind this page"), h("button", { class: "ax-btn ghost", onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, q.what, h("span", {}, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`)),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", {}, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(q.params)}` : ""))));
  }
  drawSql();

  const PAGES = [["", "Search"], ["hot", "Rising"], ["trends", "Trends"], ["atlas", "Atlas"], ["foryou", "For you"], ["feeds", "Feeds"], ["library", "Library"], ["about", "How it works"]];
  function drawNav(page) {
    fill(nav, ...PAGES.map(([p, t]) => h("a", { href: app.href(p), class: (page === p || (p === "" && ["search", "paper", "author", "topic", "concept", "category", "venue"].includes(page))) || (p === "feeds" && ["feed", "issue"].includes(page)) ? "on" : "" }, t)), h("span", { class: "sp" }), live);
  }

  // The box: typing searches after a pause; Enter or a completion commits.
  let typed = 0, suggs = [], sel = -1, lastQ = "";
  function search(q, push = true) {
    const path = q.trim() ? ["search", q.trim()] : [""];
    hideSugg();
    if (push) app.go(...path);
    else { history.replaceState(null, "", app.href(...path)); show(path); }
  }
  function hideSugg() { sugg.classList.remove("on"); sel = -1; }
  async function suggest() {
    const q = input.value;
    const me = ++typed;
    if (q.trim().length < 2) { hideSugg(); return; }
    let j;
    try { j = await ctx.api(`/v1/suggest?q=${enc(q)}`); } catch { return; }
    if (me !== typed || document.activeElement !== input) return;
    suggs = [];
    for (const x of j.questions || []) suggs.push({ k: "ask", t: x, go: () => search(x) });
    for (const x of j.phrases || []) suggs.push({ k: x.fixed ? "did you mean" : "search", t: x.text, m: x.n ? `${short(x.n)} titles` : "", go: () => search(x.text) });
    for (const x of j.concepts || []) suggs.push({ k: "concept", t: x.name, m: `${short(x.n)} papers`, go: () => { hideSugg(); app.go("concept", x.id); } });
    for (const x of j.people || []) suggs.push({ k: "person", t: x.name, m: `${fmt(x.n)} papers`, go: () => { hideSugg(); app.go("author", x.id); } });
    for (const x of j.categories || []) suggs.push({ k: "category", t: x.name, m: x.id, go: () => { hideSugg(); app.go("category", x.id); } });
    for (const x of j.papers || []) suggs.push({ k: "paper", t: x.title, m: `${x.year} · ${short(x.cites)} cites`, go: () => { hideSugg(); app.go("paper", x.id); } });
    sel = -1;
    drawSugg();
  }
  function drawSugg() {
    if (!suggs.length) { hideSugg(); return; }
    fill(sugg, suggs.map((s, i) => h("div", { class: `it${i === sel ? " sel" : ""}`, onmousedown: (e) => { e.preventDefault(); s.go(); } }, h("span", { class: "k" }, s.k), h("span", { class: "t" }, s.t), s.m ? h("span", { class: "m" }, s.m) : null)));
    sugg.classList.add("on");
  }
  let timer;
  input.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(suggest, 90); });
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" && suggs.length) { sel = (sel + 1) % suggs.length; drawSugg(); e.preventDefault(); }
    else if (e.key === "ArrowUp" && suggs.length) { sel = (sel - 1 + suggs.length) % suggs.length; drawSugg(); e.preventDefault(); }
    else if (e.key === "Enter") { e.preventDefault(); if (sel >= 0 && suggs[sel]) suggs[sel].go(); else search(input.value); }
    else if (e.key === "Escape") { hideSugg(); }
  });
  input.addEventListener("blur", () => setTimeout(hideSugg, 120));
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
        case "search": input.value = arg; lastQ = arg; return await searchPage(app, main, arg, alive, search);
        case "paper": return await paperPage(app, main, arg, alive);
        case "author": return await authorPage(app, main, arg, alive);
        case "topic": return await scopePage(app, main, "topic", arg, alive);
        case "concept": return await scopePage(app, main, "concept", arg, alive);
        case "category": return await scopePage(app, main, "category", arg, alive);
        case "venue": return await scopePage(app, main, "venue", arg, alive);
        case "trends": return await trendsPage(app, main, arg, alive);
        case "hot": return await hotPage(app, main, arg, alive);
        case "atlas": return await atlasPage(app, main, alive);
        case "foryou": return await forYouPage(app, main, alive);
        case "library": return await libraryPage(app, main, alive);
        case "feeds": return await feedsPage(app, main, alive);
        case "feed": return await studioPage(app, main, arg, alive);
        case "issue": return await issuePage(app, main, arg, alive);
        case "about": return aboutPage(app, main);
        default: fill(main, h("div", { class: "ax-empty" }, "No such page."));
      }
    } catch (e) {
      if (alive()) fill(main, h("div", { class: "ax-err" }, String(e.message || e)));
    }
  }
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) show(p); });
  // Names of techniques and categories, for chips.
  ctx.api("/v1/status").then((s) => {
    const l = s.live;
    fill(live, l ? [h("span", { class: "ax-dot" }), l.last_round ? `live · ${fmt(l.last_round.new)} new ${ago(l.last_round.at)}` : "live"] : [s.papers?.last ? `papers to ${day(s.papers.last)}` : ""]);
  }).catch(() => {});
  show(ctx.path());
}

// ------------------------------------------------------------------ home

const TRY = [
  "retrieval augmented generation",
  "who writes most about graph neural networks",
  "most cited papers on diffusion models",
  "is mamba growing",
  "LoRA vs QLoRA vs DoRA",
  "surveys of LLM agents since 2024",
  "how many papers on RLHF per year",
  "what's hot in cs.IR",
  "who introduced the transformer",
  "summarize speculative decoding",
  "reading list for recommender systems",
  "sequential recommendation with code",
  "black hole information paradox by Hawking",
  "datasets for medical image segmentation",
];

async function home(app, main, alive) {
  const stats = h("div", { class: "ax-stats" });
  const tryRow = h("div", { class: "ax-try" }, TRY.map((q) => h("a", { href: app.href("search", q) }, q)));
  const body = h("div", {}, skel(6));
  fill(main, h("div", { class: "ax-hero" },
    h("h1", {}, "Every arXiv paper, ", h("span", {}, "understood"), "."),
    h("p", {}, "Search by words and meaning, ask questions in plain English and get answers computed in SQL, follow citations, see what is rising, and get a weekly digest written for you."),
    stats, tryRow), body);
  const [j, st] = await Promise.all([app.api("/v1/home"), app.ctx.api("/v1/status").catch(() => ({}))]);
  if (!alive()) return;
  const t = st.tables || {};
  fill(stats,
    h("span", {}, h("b", {}, short(t.ax_papers ?? j.papers)), "papers"),
    t.ax_authors ? h("span", {}, h("b", {}, short(t.ax_authors)), "authors") : null,
    t.ax_cites ? h("span", {}, h("b", {}, short(t.ax_cites)), "citation links") : null,
    t.ax_events ? h("span", {}, h("b", {}, short(t.ax_events)), "reading events") : null,
    t.ax_topics ? h("span", {}, h("b", {}, fmt(t.ax_topics)), "topics") : null,
    h("span", {}, "newest ", h("b", { style: { fontSize: "14px" } }, day(j.last))));
  const conceptRows = (j.concepts || []).map((c) => [h("a", { href: app.href("concept", c.id) }, c.name), h("span", { class: `g${c.growth < 1 ? " down" : ""}` }, `${c.growth >= 1 ? "+" : ""}${Math.round((c.growth - 1) * 100)}%`), h("div", { class: "s" }, `${fmt(c.recent)} papers in the last year`)]);
  const topicRows = (j.topics || []).map((t) => [h("a", { href: app.href("topic", t.id) }, t.label), h("span", { class: "g" }, `+${Math.round((t.growth - 1) * 100)}%`), h("div", { class: "s" }, t.about || t.field_label || "")]);
  fill(body,
    h("div", { class: "ax-cols" },
      h("div", {}, sec("Newest on arXiv", h("a", { href: app.href("search", "latest papers") }, "All new")), paperList(app, (j.newest || []).slice(0, 10), { compact: true })),
      h("div", {}, sec("Most cited of the last six months"), paperList(app, (j.rising || []).slice(0, 10), { compact: true })),
      h("div", {},
        sec("Techniques rising", h("a", { href: app.href("hot") }, "More")), h("div", { class: "ax-rise" }, conceptRows),
        sec("Topics rising", h("a", { href: app.href("atlas") }, "Atlas")), h("div", { class: "ax-rise" }, topicRows))));
}

// ---------------------------------------------------------------- search

const ANSWER_NAMES = { top: "Answer", count: "How many", people: "Who writes on it", coauthors: "Who they write with", trend: "Its trend", compare: "Side by side", hot: "Rising", origin: "Where it began", survey: "Surveys", summary: "Summary", venues: "Where it is published", techniques: "Techniques used", domains: "Where it is applied", reading_list: "Reading list" };

function answerCard(app, a, und) {
  if (!a) return null;
  const kids = [h("div", { class: "ey" }, ANSWER_NAMES[a.kind] || "Answer", h("span", { class: "nm" }, "computed in SQL, no model"))];
  if (a.text && a.kind !== "summary") kids.push(h("div", { class: "big" }, a.text));
  switch (a.kind) {
    case "count": case "trend":
      kids.push(h("div", { style: { marginTop: "8px" } }, a.months && a.months.length ? lineChart([{ name: "papers a month", points: a.months.map((m, i) => [i, m.n]) }], { fy: short }) : sharesChart(a.years)));
      break;
    case "compare":
      kids.push(lineChart((a.terms || []).map((t) => ({ name: t.term, points: (t.years || []).filter((y) => y.year >= 2000).map((y) => [y.year, y.per10k]), extra: (p) => { const y = t.years.find((q) => q.year === p[0]); return y ? `${fmt(y.n)} papers` : ""; } })), { fy: (v) => v.toFixed(v < 10 ? 1 : 0), ylabel: "papers in every 10,000" }));
      break;
    case "people": case "coauthors":
      kids.push(h("ol", {}, (a.people || []).slice(0, 10).map((p) => h("li", {}, h("a", { class: "who", href: app.href("author", p.id) }, p.name), h("span", { class: "n" }, a.kind === "coauthors" ? `${p.n} together` : a.by === "cites" ? `${fmt(p.cites)} cites · ${p.n} papers` : `${p.n} papers · ${short(p.cites)} cites`)))));
      break;
    case "hot":
      kids.push(h("div", { class: "cols" },
        h("div", {}, h("h5", {}, "Techniques"), h("div", { class: "ax-rise" }, (a.techniques || []).slice(0, 8).map((t) => [h("a", { href: app.href("concept", t.id) }, t.name), h("span", { class: "g" }, `${t.growth}×`)]))),
        h("div", {}, h("h5", {}, "Domains"), h("div", { class: "ax-rise" }, (a.domains || []).slice(0, 8).map((t) => [h("a", { href: app.href("concept", t.id) }, t.name), h("span", { class: "g" }, `${t.growth}×`)]))),
        h("div", {}, h("h5", {}, "Topics"), h("div", { class: "ax-rise" }, (a.topics || []).slice(0, 8).map((t) => [h("a", { href: app.href("topic", t.id) }, t.label), h("span", { class: "g" }, `${Number(t.growth).toFixed(2)}×`)])))));
      break;
    case "origin":
      kids.push(h("div", { class: "cols" }, h("div", {}, h("h5", {}, "The earliest"), paperList(app, a.first, { compact: true })), h("div", {}, h("h5", {}, "The first cited 100+ times"), paperList(app, a.famous, { compact: true }))));
      break;
    case "venues":
      kids.push(h("ol", {}, (a.venues || []).slice(0, 10).map((v) => h("li", {}, h("a", { class: "who", href: app.href("venue", v.venue) }, v.venue), h("span", { class: "n" }, `${fmt(v.n)} papers`)))));
      break;
    case "techniques": case "domains":
      kids.push(h("ol", {}, (a.items || []).slice(0, 10).map((v) => h("li", {}, h("a", { class: "who", href: app.href("concept", v.id) }, v.name), h("span", { class: "n" }, `${v.share}% · ${fmt(v.n)}`)))));
      break;
    case "reading_list":
      kids.push(...(a.steps || []).map((s) => h("div", { style: { marginTop: "8px" } }, h("h5", { style: { margin: "6px 0 0", fontSize: "11px", textTransform: "uppercase", letterSpacing: ".06em", color: "var(--muted)" } }, s.step), paperList(app, s.papers, { compact: true }))));
      break;
    case "summary": {
      const slot = h("div", {}, h("div", { class: "ax-prog" }, h("div")), h("div", { style: { color: "var(--muted)", fontSize: "13px" } }, "Reading the most relevant abstracts and writing a summary with every claim cited…"));
      kids[0].lastChild.textContent = "written by a model from the abstracts below, each claim cited";
      kids.push(slot);
      app.api(`/v1/summary?q=${enc(a.subject)}`, { timeout: 90000 }).then((s) => {
        fill(slot, s.answer ? richText(app, s.answer, s.sources) : h("div", { class: "ax-empty" }, s.note || "No summary."), s.sources && s.sources.length ? h("details", {}, h("summary", { style: { cursor: "pointer", color: "var(--muted)", fontSize: "12.5px", marginTop: "6px" } }, `${s.sources.length} sources${s.kept ? " · kept from before" : ""}`), h("div", { class: "ax-src" }, s.sources.map((x) => [h("span", {}, `[${x.n}]`), h("a", { href: app.href("paper", x.id) }, `${x.title} (${x.year})`)]))) : null);
      }).catch((e) => fill(slot, h("div", { class: "ax-err" }, String(e.message || e))));
      break;
    }
  }
  return h("div", { class: "ax-ans" }, kids);
}

function understood(app, und, q, search) {
  const notes = (und && und.notes) || [];
  if (!notes.length) return null;
  const drop = (n) => {
    let s = q;
    if (n.from) s = s.replace(n.from, " ");
    search(s.replace(/\s+/g, " ").trim());
  };
  return h("div", { class: "ax-und" }, h("span", {}, "Understood:"), notes.map((n) => h("span", { class: "ax-chip f", title: n.from ? `from “${n.from}”` : "", onclick: () => drop(n) }, n.what, h("span", { class: "x" }, "×"))), und.text ? h("span", {}, "about ", h("b", { style: { color: "var(--text)" } }, und.text)) : null);
}

async function searchPage(app, main, q, alive, search) {
  const params = new URLSearchParams(location.hash.split("?")[1] || "");
  let sort = params.get("sort") || "", mode = params.get("mode") || "", page = 0;
  const left = h("aside", { class: "ax-left" }, skel(8));
  const center = h("section", {}, skel(10));
  const right = h("aside", { class: "ax-right" });
  fill(main, h("div", { class: "ax-grid" }, left, center, right));
  const addWords = (w) => search(`${q} ${w}`);
  async function run() {
    const t0 = performance.now();
    const j = await app.api(`/v1/search?q=${enc(q)}&sort=${enc(sort)}&mode=${enc(mode)}&page=${page}&facets=${page ? 0 : 1}`);
    if (!alive()) return;
    if (j.open && j.open.paper) { app.go("paper", j.open.paper); return; }
    const f = j.facets;
    if (f) {
      (f.technique || []).concat(f.domain || []).forEach((x) => { app.concepts[x.value] = x.name; });
      (f.category || []).forEach((x) => { CAT_NAMES[x.value] = x.name; });
      const facet = (title, rows, onpick, name = (x) => x.name || x.value) => rows && rows.length ? h("div", { class: "ax-facet" }, h("h4", {}, title), rows.slice(0, 10).map((x) => h("div", { class: "r", onclick: () => onpick(x), title: name(x) }, h("span", { class: "l" }, name(x)), h("span", { class: "n" }, short(x.n))))) : null;
      fill(left,
        f.year && f.year.length ? h("div", { class: "ax-facet" }, h("h4", {}, "Years"), yearBars(f.year, (y) => addWords(`year:${y}`))) : null,
        facet("Categories", f.category, (x) => addWords(`cat:${x.value}`), (x) => `${x.value} · ${x.name}`),
        facet("Techniques", f.technique, (x) => addWords(`technique:${x.value}`)),
        facet("Domains", f.domain, (x) => addWords(`domain:${x.value}`)),
        facet("Kind", f.kind, (x) => addWords(`kind:${x.value}`)),
        f.code ? h("div", { class: "ax-facet" }, h("div", { class: "r", onclick: () => addWords("has:code") }, h("span", { class: "l" }, "With code"), h("span", { class: "n" }, short(f.code)))) : null,
        facet("Venues", f.venue, (x) => addWords(`venue:${x.value.replace(/\s+/g, "_")}`)));
      // People: names from the authors facet keys.
      const ppl = (f.author || []).slice(0, 10);
      fill(right,
        h("div", { class: "ax-card" }, h("h4", {}, "Who writes on this", h("a", { href: app.href("search", `who writes most about ${j.understood?.text || q}`) }, "More")),
          h("div", { class: "ax-people" }, ppl.map((p) => [h("a", { href: app.href("author", p.value) }, p.value.replace(/\b\w/g, (c) => c.toUpperCase())), h("span", { class: "n" }, `${p.n}`)]))),
        j.understood?.text ? h("div", { class: "ax-card" }, h("h4", {}, "Go further"),
          h("div", { style: { display: "flex", flexDirection: "column", gap: "6px" } },
            h("a", { class: "ax-btn", href: app.href("search", `summarize ${j.understood.text}`) }, "✦ Summarize the research"),
            h("a", { class: "ax-btn", href: app.href("search", `reading list for ${j.understood.text}`) }, "Reading list"),
            h("a", { class: "ax-btn", href: app.href("trends", j.understood.text) }, "Trend over the years"),
            h("a", { class: "ax-btn", href: app.href("search", `first papers on ${j.understood.text}`) }, "Where it began"),
            h("a", { class: "ax-btn", href: app.href("feed", "new", j.understood.text) }, "Make a weekly feed of it"))) : null);
    }
    const ms = Math.round(j.took_ms ?? performance.now() - t0);
    const sortSeg = h("div", { class: "ax-seg" }, [["", "Best"], ["date", "Newest"], ["cites", "Most cited"], ["old", "Oldest"]].map(([v, t]) => h("button", { class: (j.sort === v || (!v && j.sort === "best")) ? "on" : "", onclick: () => { sort = v; page = 0; run(); } }, t)));
    const modeSeg = h("div", { class: "ax-seg" }, [["", "Words + meaning"], ["words", "Words"], ["meaning", "Meaning"], ["exact", "Exact"]].map(([v, t]) => h("button", { class: (j.mode === v || (!v && j.mode === "hybrid")) ? "on" : "", onclick: () => { mode = v; page = 0; run(); } }, t)));
    fill(center,
      understood(app, j.understood, q, search),
      j.did_you_mean ? h("div", { class: "ax-dym" }, "Did you mean ", h("a", { href: app.href("search", j.did_you_mean) }, j.did_you_mean), "?") : null,
      page === 0 ? answerCard(app, j.answer, j.understood) : null,
      h("div", { class: "ax-head" }, h("span", { class: "t" }, h("b", {}, fmt(j.total)), ` papers · ${ms} ms`), h("span", { class: "sp" }), sortSeg, modeSeg),
      paperList(app, j.papers),
      h("div", { class: "ax-pg" },
        page > 0 ? h("button", { class: "ax-btn", onclick: () => { page--; run(); } }, "← Previous") : null,
        h("span", {}, `Page ${page + 1}`),
        (j.papers || []).length >= j.per_page ? h("button", { class: "ax-btn", onclick: () => { page++; run(); scrollTo({ top: 0 }); } }, "Next →") : null));
  }
  await run();
}

// ----------------------------------------------------------------- paper

function event(app, paper, kind) {
  app.ctx.api("/v1/event", { body: { user: app.user, paper, kind } }).catch(() => {});
}

async function paperPage(app, main, id, alive) {
  fill(main, skel(12));
  const j = await app.api(`/v1/paper/${id}`);
  if (!alive()) return;
  event(app, id, "open");
  const p = j.paper;
  (j.techniques || []).concat(j.domains || []).forEach((x) => { app.concepts[x.id] = x.name; });
  (j.categories || []).forEach((x) => { CAT_NAMES[x.id] = x.name; });
  const authors = j.paper.authors_list || [];
  let showAll = authors.length <= 12;
  const auLine = h("div", { class: "au" });
  const drawAu = () => fill(auLine, (showAll ? authors : authors.slice(0, 12)).map((a, i) => [i ? ", " : "", h("a", { href: app.href("author", a.id), title: `${a.papers} papers · h-index ${a.h}` }, a.name)]), !showAll ? h("span", { class: "more", onclick: () => { showAll = true; drawAu(); } }, ` and ${authors.length - 12} more`) : null, p.nauthors > authors.length ? ` (${p.nauthors} authors)` : "");
  drawAu();
  const tldr = h("div");
  const saveBtn = h("button", { class: "ax-btn", onclick: () => { event(app, id, "save"); saveBtn.classList.add("on"); saveBtn.textContent = "✓ In your library"; } }, "☆ Save to library");
  const tldrBtn = h("button", { class: "ax-btn", onclick: async () => {
    tldrBtn.disabled = true;
    fill(tldr, h("div", { class: "ax-prog" }, h("div")));
    try {
      const t = await app.api(`/v1/tldr/${id}`, { timeout: 60000 });
      const n = t.note || {};
      fill(tldr, h("div", { class: "ax-tldr" }, h("b", {}, "TL;DR "), n.tldr || "", n.why ? h("div", { style: { marginTop: "4px", color: "var(--text-2)" } }, n.why) : null, n.results && n.results.length ? h("ul", { style: { margin: "6px 0 0", paddingLeft: "18px" } }, n.results.map((r) => h("li", {}, r))) : null));
    } catch (e) { fill(tldr, h("div", { class: "ax-err" }, String(e.message || e))); }
  } }, "✦ TL;DR");
  const tabsDef = [
    ["read_next", "Read next", j.read_next, "Fused by rank: papers cited together with it, the nearest by meaning, and PageRank from it over the citation graph."],
    ["cited_with", "Cited with it", j.cited_with, "What works that cite this paper also cite: the reading recommender's counts."],
    ["similar", "Similar", j.similar, "The nearest by meaning: the title and abstract's vectors."],
    ["references", "References", j.references, "The arXiv papers it cites, most cited first."],
    ["cited_by", "Cited by", j.cited_by, "The arXiv papers that cite it, most cited first."],
  ];
  let tab = "read_next";
  const tabs = h("div", { class: "ax-tabs" });
  const tabBody = h("div");
  const drawTabs = () => {
    fill(tabs, tabsDef.map(([k, t, rows]) => h("button", { class: k === tab ? "on" : "", onclick: () => { tab = k; drawTabs(); } }, t, h("span", { class: "n" }, (rows || []).length >= 40 ? "40+" : String((rows || []).length)))));
    const d = tabsDef.find((x) => x[0] === tab);
    fill(tabBody, h("div", { style: { fontSize: "12.5px", color: "var(--muted)", margin: "6px 0" } }, d[3]), paperList(app, d[2]));
  };
  drawTabs();
  const kinds = (p.kinds || []).filter((k) => k !== "theory");
  fill(main, h("div", { class: "ax-grid two" },
    h("section", {},
      h("div", { class: "ax-ph" },
        h("div", { class: "ey" },
          h("a", { href: `https://arxiv.org/abs/${p.id}`, target: "_blank", rel: "noopener", style: { fontFamily: "var(--mono)" } }, `arXiv:${p.id}`),
          (p.cats || []).map((c) => catChip(app, c)),
          h("span", {}, `${day(p.created)}${p.versions > 1 ? ` · v${p.versions}, ${day(p.updated)}` : ""}`),
          kinds.map((k) => h("span", { class: `ax-badge ${k}` }, k))),
        h("h1", {}, p.title),
        auLine,
        h("div", { class: "acts" },
          h("a", { class: "ax-btn pri", href: j.links.pdf, target: "_blank", rel: "noopener", onclick: () => event(app, id, "pdf") }, "PDF"),
          h("a", { class: "ax-btn", href: j.links.html, target: "_blank", rel: "noopener" }, "HTML"),
          h("a", { class: "ax-btn", href: j.links.abs, target: "_blank", rel: "noopener" }, "arXiv"),
          p.code ? h("a", { class: "ax-btn", href: p.code, target: "_blank", rel: "noopener" }, "Code") : null,
          p.hf ? h("a", { class: "ax-btn", href: p.hf, target: "_blank", rel: "noopener" }, "🤗 Hugging Face") : null,
          tldrBtn, saveBtn),
        tldr,
        h("div", { class: "abs" }, p.abstract),
        h("div", { class: "tags" }, (j.techniques || []).map((x) => conceptChip(app, x.id, x.name, "technique")), (j.domains || []).map((x) => conceptChip(app, x.id, x.name, "domain")))),
      tabs, tabBody),
    h("aside", {},
      h("div", { class: "ax-kpis" },
        h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(p.cites)), h("div", { class: "l" }, "citations")),
        h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(p.cited_in)), h("div", { class: "l" }, "on arXiv")),
        h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(p.refs)), h("div", { class: "l" }, "references"))),
      j.topic ? h("div", { class: "ax-card" }, h("h4", {}, "Topic", h("a", { href: app.href("atlas") }, "Atlas")), h("div", {}, h("a", { href: app.href("topic", j.topic.id), style: { fontWeight: 600 } }, j.topic.label)), h("div", { style: { fontSize: "13px", color: "var(--text-2)", marginTop: "3px" } }, j.topic.about), h("div", { style: { fontSize: "12px", color: "var(--muted)", marginTop: "5px" } }, `${fmt(j.topic.papers)} papers · ${j.topic.field_label}`)) : null,
      p.venue || p.journal ? h("div", { class: "ax-card" }, h("h4", {}, "Published"), p.venue ? h("a", { href: app.href("venue", p.venue) }, `${p.venue}${p.venue_year ? " " + p.venue_year : ""}`) : null, p.journal ? h("div", { style: { fontSize: "13px", color: "var(--text-2)" } }, p.journal) : null) : null,
      h("div", { class: "ax-card" }, h("h4", {}, "Details"),
        h("div", { style: { fontSize: "13px", color: "var(--text-2)", display: "grid", gap: "3px" } },
          p.comments ? h("div", {}, p.comments) : null,
          p.doi ? h("div", {}, "DOI ", h("a", { href: `https://doi.org/${p.doi}`, target: "_blank", rel: "noopener", style: { color: "var(--accent)" } }, p.doi)) : null,
          p.license ? h("div", {}, `License: ${p.license}`) : null)),
      j.after && j.after.length ? h("div", { class: "ax-card" }, h("h4", {}, "Read after it"), h("div", { style: { fontSize: "12px", color: "var(--muted)", marginBottom: "4px" } }, "What came next in readers' lists"), paperList(app, j.after.slice(0, 5), { compact: true })) : null)));
}

// ---------------------------------------------------------------- author

async function authorPage(app, main, id, alive) {
  fill(main, skel(12));
  const j = await app.api(`/v1/author/${enc(id)}`);
  if (!alive()) return;
  const a = j.author;
  (j.techniques || []).forEach((x) => { app.concepts[x.id] = x.name; });
  const years = (j.years || []).filter((y) => y.year > 1990);
  fill(main, h("div", { class: "ax-grid two" },
    h("section", {},
      h("div", { class: "ax-ph" },
        h("div", { class: "ey" }, "Author", (j.categories || []).slice(0, 4).map((c) => catChip(app, c.id))),
        h("h1", {}, a.name),
        h("div", { class: "au" }, `On arXiv ${day(a.first).slice(0, 4)}–${day(a.last).slice(0, 4)}`, j.first_author ? ` · first author of ${fmt(j.first_author)}` : ""),
        h("div", { class: "ax-kpis" },
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(a.papers)), h("div", { class: "l" }, "papers")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(a.cites)), h("div", { class: "l" }, "citations")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(a.h)), h("div", { class: "l" }, "h-index")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(a.recent)), h("div", { class: "l" }, "last 12 months")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(a.coauthors)), h("div", { class: "l" }, "coauthors")))),
      years.length > 1 ? [sec("Papers a year"), lineChart([{ name: "papers", points: years.map((y) => [y.year, y.n]), extra: (p) => { const y = years.find((q) => q.year === p[0]); return y ? `${fmt(y.cites)} cites` : ""; } }], { fy: (v) => String(Math.round(v)) })] : null,
      sec("Most cited"), paperList(app, j.top, { compact: false }),
      sec("Newest"), paperList(app, j.newest, { compact: true })),
    h("aside", {},
      h("div", { class: "ax-card" }, h("h4", {}, "Writes most with"), peopleList(app, (j.coauthors || []).slice(0, 14), { n: "n", label: "together" })),
      h("div", { class: "ax-card" }, h("h4", {}, "Nearest in the coauthor graph"), h("div", { style: { fontSize: "12px", color: "var(--muted)", marginBottom: "4px" } }, "PageRank from them over who writes with whom"), peopleList(app, (j.orbit || []).slice(0, 10), { n: "papers", label: "papers" })),
      j.topics && j.topics.length ? h("div", { class: "ax-card" }, h("h4", {}, "Topics"), h("div", { class: "ax-people" }, j.topics.map((t) => [h("a", { href: app.href("topic", t.id) }, t.label), h("span", { class: "n" }, String(t.n))]))) : null,
      j.techniques && j.techniques.length ? h("div", { class: "ax-card" }, h("h4", {}, "Techniques"), conceptList(app, j.techniques)) : null)));
}

// -------------------------------------------- topic, concept, category, venue

async function scopePage(app, main, kind, id, alive) {
  fill(main, skel(12));
  const path = kind === "topic" ? `/v1/topic/${enc(id)}` : kind === "concept" ? `/v1/concept/${enc(id)}` : kind === "category" ? `/v1/category/${enc(id)}` : `/v1/venue/${enc(id)}`;
  const j = await app.api(path);
  if (!alive()) return;
  const b = j.body || {};
  (b.techniques || []).concat(b.domains || []).forEach((x) => { app.concepts[x.id] = x.name; });
  let title, eyebrow, about = null, query, extra = null;
  if (kind === "topic") {
    title = j.topic.label; eyebrow = ["Topic", h("span", {}, j.topic.field_label)]; about = j.topic.about; query = `topic:${id}`;
    extra = h("div", { class: "tags", style: { marginTop: "8px" } }, (j.topic.terms || []).slice(0, 10).map((t) => h("a", { class: "ax-chip", href: app.href("search", t) }, t)));
  } else if (kind === "concept") {
    title = j.concept.name; eyebrow = [j.concept.kind === "domain" ? "Domain" : "Technique"]; query = `${j.concept.kind === "domain" ? "domain" : "technique"}:${id}`;
    about = (j.concept.phrases || []).length ? `Found by: ${(j.concept.phrases || []).slice(0, 8).join(", ")}${(j.concept.acronyms || []).length ? "; " + j.concept.acronyms.join(", ") : ""}` : null;
  } else if (kind === "category") {
    title = j.category.name; eyebrow = ["Category", h("span", { style: { fontFamily: "var(--mono)" } }, j.category.id)]; query = `cat:${id}`;
  } else {
    title = j.venue; eyebrow = ["Venue"]; query = `venue:${id.replace(/\s+/g, "_")}`;
  }
  const g = b.before ? b.recent / b.before : null;
  fill(main, h("div", { class: "ax-grid two" },
    h("section", {},
      h("div", { class: "ax-ph" },
        h("div", { class: "ey" }, eyebrow),
        h("h1", {}, title),
        about ? h("div", { class: "au" }, about) : null, extra,
        h("div", { class: "ax-kpis" },
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(b.total)), h("div", { class: "l" }, "papers")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(b.recent)), h("div", { class: "l" }, "last 12 months")),
          g != null ? h("div", { class: "ax-kpi" }, h("div", { class: "v", style: { color: g >= 1 ? "var(--good-ink)" : "var(--crit-ink)" } }, `${g >= 1 ? "+" : ""}${Math.round((g - 1) * 100)}%`), h("div", { class: "l" }, "vs the year before")) : null),
        h("div", { class: "acts" },
          h("a", { class: "ax-btn pri", href: app.href("search", `summarize ${kind === "topic" ? j.topic.label : title}`) }, "✦ Summarize the research"),
          h("a", { class: "ax-btn", href: app.href("search", query) }, "Search in it"),
          h("a", { class: "ax-btn", href: app.href("search", `reading list for ${kind === "topic" ? j.topic.label : title}`) }, "Reading list"),
          h("a", { class: "ax-btn", href: app.href("feed", "new", kind === "topic" ? j.topic.label : title) }, "Weekly feed"))),
      sec("Share of arXiv, year by year"), sharesChart(b.years),
      sec("Most cited of the last twelve months"), paperList(app, b.rising, { compact: true }),
      sec("Most cited ever"), paperList(app, b.top, { compact: true }),
      sec("Newest", h("a", { href: app.href("search", `${query} sort:date`) }, "All")), paperList(app, b.newest, { compact: true })),
    h("aside", {},
      h("div", { class: "ax-card" }, h("h4", {}, "Who writes most on it"), peopleList(app, (b.people || []).slice(0, 12))),
      b.techniques && b.techniques.length ? h("div", { class: "ax-card" }, h("h4", {}, "Techniques its papers use"), conceptList(app, b.techniques.slice(0, 10))) : null,
      b.domains && b.domains.length ? h("div", { class: "ax-card" }, h("h4", {}, "Domains it serves"), conceptList(app, b.domains.slice(0, 10))) : null,
      j.near && j.near.length ? h("div", { class: "ax-card" }, h("h4", {}, "Nearest topics"), h("div", { class: "ax-people" }, j.near.map((t) => [h("a", { href: app.href("topic", t.id) }, t.label), h("span", { class: "n" }, short(t.papers))]))) : null,
      j.topics && j.topics.length ? h("div", { class: "ax-card" }, h("h4", {}, "Its topics"), h("div", { class: "ax-people" }, j.topics.map((t) => [h("a", { href: app.href("topic", t.id) }, t.label), h("span", { class: "n" }, short(t.papers))]))) : null,
      b.categories && b.categories.length ? h("div", { class: "ax-card" }, h("h4", {}, "Categories"), h("div", { class: "ax-people" }, b.categories.map((c) => [h("a", { href: app.href("category", c.id) }, `${c.id} · ${c.name}`), h("span", { class: "n" }, short(c.n))]))) : null)));
}

// ---------------------------------------------------------------- trends

async function trendsPage(app, main, terms, alive) {
  const inp = h("input", { type: "text", value: terms || "transformer, diffusion models, graph neural networks, reinforcement learning", style: { width: "100%", padding: "9px 12px", borderRadius: "9px", border: "1px solid var(--line-2)", background: "var(--surface)", color: "var(--text)", font: "14px var(--sans)" } });
  const go = () => app.go("trends", inp.value);
  inp.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
  const out = h("div", {}, skel(6));
  fill(main, h("div", { style: { maxWidth: "980px" } },
    h("div", { class: "ax-ph" }, h("h1", {}, "Trends"), h("div", { class: "au" }, "Topics side by side: each one's share of all arXiv papers, year by year, so growth is not just arXiv growing. Separate topics with commas.")),
    h("div", { style: { display: "flex", gap: "8px" } }, inp, h("button", { class: "ax-btn pri", onclick: go }, "Compare")),
    h("div", { class: "ax-try", style: { justifyContent: "flex-start" } }, ["LoRA, QLoRA, DoRA", "GAN, VAE, diffusion models, flow matching", "BERT, GPT, T5, Llama", "RNN, LSTM, transformer, state space model", "dark matter, dark energy, gravitational waves", "federated learning, differential privacy", "NeRF, gaussian splatting"].map((t) => h("a", { href: app.href("trends", t) }, t))),
    out));
  const q = inp.value;
  const j = await app.api(`/v1/trends?q=${enc(q)}`);
  if (!alive()) return;
  fill(out, h("div", { class: "ax-ans" }, h("div", { class: "ey" }, "Side by side", h("span", { class: "nm" }, "computed in SQL, no model")), h("div", { class: "big" }, j.text),
    lineChart((j.terms || []).map((t) => ({ name: t.term, points: (t.years || []).filter((y) => y.year >= 1995).map((y) => [y.year, y.per10k]), extra: (p) => { const y = t.years.find((x) => x.year === p[0]); return y ? `${fmt(y.n)} papers` : ""; } })), { height: 300, fy: (v) => v.toFixed(v < 10 ? 1 : 0), ylabel: "papers in every 10,000" })),
    h("div", { class: "ax-cols" }, (j.terms || []).map((t) => h("div", { class: "ax-card" }, h("h4", {}, t.term, h("a", { href: app.href("search", t.term) }, "Papers")), h("div", { style: { fontSize: "13.5px" } }, `${fmt(t.total)} papers · ${fmt(t.recent)} in the last year (${t.before ? `${t.recent >= t.before ? "+" : ""}${Math.round((t.recent / t.before - 1) * 100)}%` : "new"})`)))));
}

// ------------------------------------------------------------------- hot

const AREAS = [["", "All of arXiv"], ["cs", "Computer science"], ["stat", "Statistics"], ["eess", "Electrical eng."], ["math", "Mathematics"], ["physics", "Physics"], ["astro-ph", "Astrophysics"], ["cond-mat", "Condensed matter"], ["hep", "High energy physics"], ["quant-ph", "Quantum physics"], ["q-bio", "Biology"], ["q-fin", "Finance"], ["econ", "Economics"]];

async function hotPage(app, main, area, alive) {
  const out = h("div", {}, skel(8));
  fill(main, h("div", {},
    h("div", { class: "ax-ph" }, h("h1", {}, "What's rising"), h("div", { class: "au" }, "Techniques, domains and topics whose share of papers grew most in the last twelve months against the twelve before.")),
    h("div", { class: "ax-try", style: { justifyContent: "flex-start" } }, AREAS.map(([k, t]) => h("a", { href: app.href("hot", k), style: (area || "") === k ? { borderColor: "var(--accent)", color: "var(--accent)" } : {} }, t))),
    out));
  const j = await app.api(`/v1/hot?area=${enc(area || "")}`);
  if (!alive()) return;
  const col = (title, rows, kind) => h("div", { class: "ax-card" }, h("h4", {}, title), h("div", { class: "ax-rise" }, (rows || []).map((t) => [h("a", { href: kind === "topic" ? app.href("topic", t.id) : app.href("concept", t.id) }, t.name || t.label), h("span", { class: "g" }, `${Number(t.growth).toFixed(2)}×`), h("div", { class: "s" }, kind === "topic" ? (t.about || "") : `${fmt(t.recent)} papers, from ${fmt(t.before)}`)])));
  fill(out, h("div", { class: "ax-cols" }, col("Techniques", j.techniques, "c"), col("Domains", j.domains, "c"), col("Topics", j.topics, "topic")));
}

// ----------------------------------------------------------------- atlas

async function atlasPage(app, main, alive) {
  const canvas = h("canvas");
  const find = h("input", { type: "text", placeholder: "Find a topic…" });
  const list = h("div");
  const side = h("div", { class: "side" }, h("b", {}, "The atlas of arXiv"), h("div", { style: { fontSize: "12.5px", color: "var(--muted)", margin: "3px 0 8px" } }, "1,000 topics found by clustering every paper's meaning, placed so near topics are near. Color is the arXiv area, size is papers, a green ring marks a topic that grew 60% or more last year."), find, list);
  const wrap = h("div", { class: "ax-atlas" }, canvas, side, h("div", { class: "hint" }, "Scroll to zoom · drag to move · click a topic"));
  fill(main, wrap);
  const j = await app.api("/v1/atlas");
  if (!alive()) return;
  const topics = j.topics || [], fields = j.fields || [];
  const color = (f) => FIELD_COLORS[((f % FIELD_COLORS.length) + FIELD_COLORS.length) % FIELD_COLORS.length];
  let scale = 1, ox = 0, oy = 0, hover = null, mark = null;
  const ctx2 = canvas.getContext("2d");
  const maxP = Math.max(...topics.map((t) => t.papers), 1);
  function size() { const r = canvas.getBoundingClientRect(); canvas.width = r.width * devicePixelRatio; canvas.height = r.height * devicePixelRatio; }
  // The map fills the room right of the side panel (all of it on a narrow screen).
  const P = (t) => { const w = canvas.width, hh = canvas.height, m = 60 * devicePixelRatio, l = (w / devicePixelRatio > 760 ? 340 : 60) * devicePixelRatio, cx = (l + w - m) / 2; return [cx + (t.x - 0.5) * (w - l - m) * scale + ox, hh / 2 + (t.y - 0.5) * (hh - 2 * m) * scale + oy]; };
  const R = (t) => (3 + Math.sqrt(t.papers / maxP) * 26) * devicePixelRatio * Math.sqrt(scale);
  function draw() {
    const dark = matchMedia("(prefers-color-scheme: dark)").matches && document.documentElement.dataset.theme !== "light" || document.documentElement.dataset.theme === "dark";
    ctx2.clearRect(0, 0, canvas.width, canvas.height);
    for (const t of topics) {
      const [x, y] = P(t), r = R(t);
      ctx2.beginPath(); ctx2.arc(x, y, r, 0, 7);
      ctx2.fillStyle = color(t.field) + (t === hover || t === mark ? "ee" : "88"); ctx2.fill();
      if (t.growth > 1.6 && t.recent > 200) { ctx2.lineWidth = 2 * devicePixelRatio; ctx2.strokeStyle = dark ? "#7fe39a" : "#0a8a2a"; ctx2.stroke(); }
    }
    ctx2.textAlign = "center";
    const label = (text, x, y, size, weight, alpha) => {
      ctx2.font = `${weight} ${size * devicePixelRatio}px system-ui, -apple-system, sans-serif`;
      ctx2.lineJoin = "round"; ctx2.lineWidth = 4 * devicePixelRatio;
      ctx2.strokeStyle = dark ? "rgba(22,24,28,.85)" : "rgba(255,255,255,.9)"; ctx2.strokeText(text, x, y);
      ctx2.fillStyle = dark ? `rgba(236,238,241,${alpha})` : `rgba(15,18,22,${alpha})`; ctx2.fillText(text, x, y);
    };
    if (scale > 2.2) {
      const shown = topics.filter((t) => { const [x, y] = P(t); return x > 0 && y > 0 && x < canvas.width && y < canvas.height; }).sort((a, b) => b.papers - a.papers).slice(0, Math.round(18 * scale));
      for (const t of shown) { const [x, y] = P(t); label(t.label, x, y + R(t) + 11 * devicePixelRatio, 10.5, 500, .8); }
    }
    for (const f of fields) {
      if (f.topics < 3 && scale < 2) continue;
      const [x, y] = P(f);
      label(f.label, x, y, scale > 2.2 ? 15 : 13, 700, .95);
    }
    if (hover) { const [x, y] = P(hover); label(hover.label, x, y - R(hover) - 6 * devicePixelRatio, 12.5, 650, 1); }
  }
  size(); draw();
  addEventListener("resize", () => { if (alive()) { size(); draw(); } });
  const at = (ev) => { const b = canvas.getBoundingClientRect(); const mx = (ev.clientX - b.left) * devicePixelRatio, my = (ev.clientY - b.top) * devicePixelRatio; let best = null, bd = 1e9; for (const t of topics) { const [x, y] = P(t); const d = Math.hypot(x - mx, y - my); if (d < R(t) + 4 && d < bd) { bd = d; best = t; } } return best; };
  let drag = null, moved = false;
  canvas.addEventListener("mousedown", (e) => { drag = [e.clientX, e.clientY, ox, oy]; moved = false; canvas.style.cursor = "grabbing"; });
  addEventListener("mouseup", () => { drag = null; canvas.style.cursor = "grab"; });
  canvas.addEventListener("mousemove", (e) => {
    if (drag) { ox = drag[2] + (e.clientX - drag[0]) * devicePixelRatio; oy = drag[3] + (e.clientY - drag[1]) * devicePixelRatio; moved = true; draw(); return; }
    const t = at(e);
    if (t !== hover) { hover = t; draw(); }
    if (t) showTip(e, `<b>${esc(t.label)}</b>${esc(t.field_label)}<br>${fmt(t.papers)} papers · ${fmt(t.recent)} last year · ${Number(t.growth).toFixed(2)}× growth`); else hideTip();
  });
  canvas.addEventListener("mouseleave", hideTip);
  canvas.addEventListener("click", (e) => { if (moved) return; const t = at(e); if (t) app.go("topic", t.id); });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const b = canvas.getBoundingClientRect();
    const w = canvas.width, l = (w / devicePixelRatio > 760 ? 340 : 60) * devicePixelRatio, cx = (l + w - 60 * devicePixelRatio) / 2;
    const mx = (e.clientX - b.left) * devicePixelRatio - cx, my = (e.clientY - b.top) * devicePixelRatio - canvas.height / 2;
    const k = Math.exp(-e.deltaY * 0.0015), ns = Math.min(14, Math.max(0.6, scale * k)), f = ns / scale;
    ox = mx - (mx - ox) * f; oy = my - (my - oy) * f; scale = ns; draw();
  }, { passive: false });
  const drawList = () => {
    const q = find.value.trim().toLowerCase();
    const rows = (q ? topics.filter((t) => t.label.toLowerCase().includes(q) || (t.field_label || "").toLowerCase().includes(q)) : fields.slice().sort((a, b) => b.papers - a.papers).map((f) => ({ ...f, isField: true }))).slice(0, 40);
    fill(list, h("div", { class: "ax-people" }, rows.map((t) => [h("a", { href: t.isField ? "javascript:void 0" : app.href("topic", t.id), onmouseenter: () => { if (!t.isField) { mark = t; draw(); } }, onclick: (e) => { if (t.isField) { e.preventDefault(); find.value = t.label; drawList(); } }, style: t.isField ? { color: color(t.id), fontWeight: 600 } : {} }, t.label), h("span", { class: "n" }, short(t.papers))])));
  };
  find.addEventListener("input", drawList);
  drawList();
}

// --------------------------------------------------------------- for you

async function forYouPage(app, main, alive) {
  fill(main, skel(10));
  const j = await app.api(`/v1/foryou?user=${enc(app.user)}`);
  if (!alive()) return;
  const WHY = { next: "read after yours", co: "read with yours", similar: "near yours in meaning", popular: "popular" };
  const n = (j.history || []).length;
  fill(main, h("div", { class: "ax-grid two" },
    h("section", {},
      h("div", { class: "ax-ph" }, h("h1", {}, "For you"), h("div", { class: "au" }, n ? "What to read next, from the papers you opened and kept here: ChloeDB's recommender (what readers read after and with them, the nearest in meaning) fused with PageRank from them over the citation graph." : "Open or save a few papers and this page learns what to recommend: every paper you open is an event the recommender counts as it is written.")),
      n ? paperList(app, j.papers, {}) : h("div", { class: "ax-try", style: { justifyContent: "flex-start" } }, TRY.slice(0, 6).map((q) => h("a", { href: app.href("search", q) }, q)))),
    h("aside", {},
      h("div", { class: "ax-card" }, h("h4", {}, "From what you read"), n ? paperList(app, j.history, { compact: true }) : h("div", { class: "ax-empty" }, "Nothing yet.")),
      h("div", { class: "ax-card" }, h("h4", {}, "Why these"), h("div", { style: { fontSize: "13px", color: "var(--text-2)" } }, "Each paper's rank fuses four lists of the recommender (", Object.values(WHY).join(", "), ") and a walk of the citation graph from your papers. The SQL button shows the query.")))));
  // Why, on each item.
  main.querySelectorAll(".ax-p").forEach((li, i) => { const r = (j.papers || [])[i]; if (r && r.why && r.why.length) li.append(h("div", { class: "why" }, r.why.map((w) => WHY[w] || w).join(" · "))); });
}

async function libraryPage(app, main, alive) {
  fill(main, skel(8));
  const j = await app.api(`/v1/library?user=${enc(app.user)}`);
  if (!alive()) return;
  fill(main, h("div", { style: { maxWidth: "900px" } }, h("div", { class: "ax-ph" }, h("h1", {}, "Library"), h("div", { class: "au" }, "The papers you saved, kept under this browser's reader id.")), paperList(app, j.papers)));
}

// ----------------------------------------------------------------- feeds

async function feedsPage(app, main, alive) {
  fill(main, skel(6));
  const [mine, pub] = await Promise.all([app.api(`/v1/feeds?user=${enc(app.user)}`), app.ctx.api("/v1/feeds").catch(() => ({ feeds: [] }))]);
  if (!alive()) return;
  const row = (f) => h("li", { class: "ax-p compact" }, h("div", { class: "ttl" }, h("a", { href: app.href("feed", f.id) }, f.name)), h("div", { class: "meta" }, `updated ${ago(f.updated)}`, h("a", { href: `feeds/${f.id}/rss.xml`, target: "_blank" }, "RSS")));
  fill(main, h("div", { style: { maxWidth: "900px" } },
    h("div", { class: "ax-ph" }, h("h1", {}, "Feeds"), h("div", { class: "au" }, "A weekly digest of arXiv written for you, like the best research newsletters: say what you care about in your own words, and every week it picks the papers that matter most to you and writes a paragraph on each, with who it is from, what it does and what it found.")),
    h("div", { class: "acts", style: { display: "flex", gap: "8px", margin: "12px 0" } }, h("a", { class: "ax-btn pri", href: app.href("feed", "new") }, "+ New feed"), h("a", { class: "ax-btn", href: app.href("feed", "example") }, "Start from “Recommender systems weekly”")),
    sec("Your feeds"), (mine.feeds || []).length ? h("ul", { class: "ax-list" }, mine.feeds.map(row)) : h("div", { class: "ax-empty" }, "None yet."),
    (pub.feeds || []).length ? [sec("Shared feeds"), h("ul", { class: "ax-list" }, pub.feeds.map(row))] : null));
}

function tagsInput(values, onChange, placeholder) {
  const wrap = h("div", { class: "ax-tagsin" });
  const inp = h("input", { type: "text", placeholder });
  const draw = () => { fill(wrap, values.map((v, i) => h("span", { class: "ax-chip f" }, v, h("span", { class: "x", onclick: () => { values.splice(i, 1); draw(); onChange(); } }, "×"))), inp); inp.focus && null; };
  inp.addEventListener("keydown", (e) => {
    if ((e.key === "Enter" || e.key === ",") && inp.value.trim()) { e.preventDefault(); values.push(inp.value.trim().replace(/,$/, "")); inp.value = ""; draw(); onChange(); inp.focus(); }
    else if (e.key === "Backspace" && !inp.value && values.length) { values.pop(); draw(); onChange(); inp.focus(); }
  });
  inp.addEventListener("blur", () => { if (inp.value.trim()) { values.push(inp.value.trim()); inp.value = ""; draw(); onChange(); } });
  draw();
  return wrap;
}

function checks(opts, values, onChange) {
  const wrap = h("div", { class: "ax-check" });
  const draw = () => fill(wrap, opts.map(([k, t]) => { const on = values.includes(k); return h("label", { class: on ? "on" : "", onclick: (e) => { e.preventDefault(); const i = values.indexOf(k); if (i >= 0) values.splice(i, 1); else values.push(k); draw(); onChange(); } }, t); }));
  draw();
  return wrap;
}

function slider(label, min, max, step, get, set, onChange, fmtv = (v) => v) {
  const val = h("span", { class: "val" }, fmtv(get()));
  const r = h("input", { type: "range", min, max, step, value: get(), oninput: (e) => { set(Number(e.target.value)); val.textContent = fmtv(get()); onChange(); } });
  return h("div", { class: "ax-f" }, h("label", {}, label), h("div", { class: "row" }, r, val));
}

async function studioPage(app, main, arg, alive) {
  const [id0, ...seedWords] = arg.split("/");
  let feedId = id0 === "new" || id0 === "example" ? "" : id0;
  fill(main, skel(10));
  let cfg, issues = [], owner = app.user;
  if (feedId) {
    const j = await app.api(`/v1/feeds/${enc(feedId)}`);
    cfg = j.config; issues = j.issues || []; owner = j.owner;
  } else if (id0 === "example") {
    cfg = (await app.ctx.api("/v1/feeds/example")).config;
  } else {
    cfg = (await app.ctx.api("/v1/feeds/example")).config;
    const topic = seedWords.join("/");
    Object.assign(cfg, { name: topic ? `${topic.replace(/\b\w/, (c) => c.toUpperCase())} weekly` : "My arXiv digest", tagline: topic ? `The week's most useful papers on ${topic}` : "", interests: topic ? [topic] : [], categories: [] });
  }
  if (!alive()) return;
  const mine = owner === app.user;
  const preview = h("div", { class: "ax-prev" }, skel(8));
  const status = h("div");
  let ptimer, pseq = 0;
  const changed = () => { clearTimeout(ptimer); ptimer = setTimeout(runPreview, 350); };
  async function runPreview() {
    const me = ++pseq;
    preview.style.opacity = ".55";
    try {
      const j = await app.api("/v1/feeds/preview", { body: { config: cfg } });
      if (me !== pseq || !alive()) return;
      preview.style.opacity = "1";
      const FOUND = (f) => f.startsWith("interest:") ? `“${f.slice(9)}”` : f.startsWith("like:") ? `like ${f.slice(5)}` : f;
      fill(preview,
        h("div", { class: "ax-head" }, h("span", { class: "t" }, h("b", {}, `Vol. ${issues.length + 1}`), ` · ${j.span} · ${j.papers.length} papers from ${fmt(j.candidates)} candidates · ${Math.round(j.took_ms)} ms`)),
        h("ul", { class: "ax-list" }, (j.papers || []).map((r, i) => { const li = paperItem(app, r, {}); li.querySelector(".ttl").prepend(h("span", { style: { color: "var(--muted)", marginRight: "6px" } }, `${i + 1}.`)); li.append(h("div", { class: "found" }, "found by ", (r.found_by || []).map((f) => h("span", {}, FOUND(f))), ` · score ${r.feed_score}`)); return li; })),
        ["tools", "benchmarks", "surveys"].map((k) => (j[k] || []).length ? [sec(`Extras: ${k[0].toUpperCase() + k.slice(1)}`), paperList(app, j[k], { compact: true })] : null));
    } catch (e) { if (me === pseq) { preview.style.opacity = "1"; fill(preview, h("div", { class: "ax-err" }, String(e.message || e))); } }
  }
  const nameIn = h("input", { type: "text", value: cfg.name, oninput: (e) => { cfg.name = e.target.value; } });
  const tagIn = h("input", { type: "text", value: cfg.tagline, oninput: (e) => { cfg.tagline = e.target.value; } });
  const audIn = h("select", { onchange: (e) => { cfg.audience = e.target.value; } }, ["practitioners and researchers", "machine learning engineers", "researchers", "executives and investors", "students"].map((a) => h("option", { value: a, selected: cfg.audience === a ? "" : null }, a)));
  const saveBtn = h("button", { class: "ax-btn", disabled: !mine && !!feedId }, feedId ? "Save" : "Save feed");
  const writeBtn = h("button", { class: "ax-btn pri" }, "✦ Write this week's issue");
  saveBtn.onclick = async () => {
    const j = await app.api("/v1/feeds", { body: { id: feedId, user: app.user, config: cfg } });
    if (!feedId) { feedId = j.id; history.replaceState(null, "", app.href("feed", feedId)); }
    saveBtn.textContent = "Saved ✓"; setTimeout(() => (saveBtn.textContent = "Save"), 1500);
  };
  writeBtn.onclick = async () => {
    if (!feedId) await saveBtn.onclick();
    else await app.api("/v1/feeds", { body: { id: feedId, user: app.user, config: cfg } });
    writeBtn.disabled = true;
    fill(status, h("div", { class: "ax-card" }, h("div", { class: "ax-prog" }, h("div")), h("div", { class: "step", style: { fontSize: "13px", color: "var(--text-2)" } }, "Starting…")));
    try {
      const { job } = await app.api(`/v1/feeds/${enc(feedId)}/issue`, { body: { user: app.user } });
      for (;;) {
        await new Promise((r) => setTimeout(r, 1500));
        const s = await app.ctx.api(`/v1/jobs/${job}`);
        if (!alive()) return;
        if (s.state === "running") { const st = status.querySelector(".step"); if (st) st.textContent = s.step; continue; }
        if (s.state === "done") { app.go("issue", s.issue); return; }
        fill(status, h("div", { class: "ax-err" }, s.error || "failed")); writeBtn.disabled = false; return;
      }
    } catch (e) { fill(status, h("div", { class: "ax-err" }, String(e.message || e))); writeBtn.disabled = false; }
  };
  const w = cfg.weights || (cfg.weights = { relevance: 1, code: 0.25, citations: 0.15, authors: 0.2 });
  const form = h("div", { class: "ax-form" },
    h("div", { class: "ax-f" }, h("label", {}, "Name"), nameIn),
    h("div", { class: "ax-f" }, h("label", {}, "Tagline"), tagIn),
    h("div", { class: "ax-f" }, h("label", {}, "What you care about ", h("small", {}, "in your own words; Enter after each")), tagsInput(cfg.interests, changed, "e.g. sequential recommendation")),
    h("div", { class: "ax-f" }, h("label", {}, "Papers like these ", h("small", {}, "arXiv ids")), tagsInput(cfg.seeds, changed, "e.g. 2305.19860")),
    h("div", { class: "ax-f" }, h("label", {}, "Categories ", h("small", {}, "e.g. cs.IR")), tagsInput(cfg.categories, changed, "cs.IR")),
    h("div", { class: "ax-f" }, h("label", {}, "Techniques and domains ", h("small", {}, "slugs, e.g. rag, diffusion")), tagsInput(cfg.techniques, changed, "technique")),
    h("div", { class: "ax-f" }, h("label", {}, "Always include people ", h("small", {}, "author keys from their pages")), tagsInput(cfg.authors, changed, "e.g. julian mcauley")),
    h("div", { class: "ax-f" }, h("label", {}, "Leave out papers about"), tagsInput(cfg.exclude, changed, "e.g. medical")),
    h("div", { class: "ax-f" }, h("label", {}, "Only kinds ", h("small", {}, "none: every paper")), checks([["survey", "Surveys"], ["benchmark", "Benchmarks"], ["dataset", "Datasets"], ["tool", "Tools"]], cfg.kinds, changed)),
    h("div", { class: "ax-f" }, h("label", { class: "ax-check" }, h("label", { class: cfg.code_only ? "on" : "", onclick: (e) => { e.preventDefault(); cfg.code_only = !cfg.code_only; e.currentTarget.classList.toggle("on"); changed(); } }, "Only papers with code"))),
    slider("Days an issue covers", 1, 31, 1, () => cfg.days, (v) => (cfg.days = v), changed),
    slider("Papers", 3, 25, 1, () => cfg.papers, (v) => (cfg.papers = v), changed),
    slider("At most per topic", 1, 6, 1, () => cfg.per_topic, (v) => (cfg.per_topic = v), changed),
    h("div", { class: "ax-f" }, h("label", {}, "Weights"),
      slider("Relevance", 0, 2, 0.05, () => w.relevance, (v) => (w.relevance = v), changed, (v) => v.toFixed(2)),
      slider("Has code", 0, 1, 0.05, () => w.code, (v) => (w.code = v), changed, (v) => v.toFixed(2)),
      slider("Citations so far", 0, 1, 0.05, () => w.citations, (v) => (w.citations = v), changed, (v) => v.toFixed(2)),
      slider("Established authors", 0, 1, 0.05, () => w.authors, (v) => (w.authors = v), changed, (v) => v.toFixed(2))),
    h("div", { class: "ax-f" }, h("label", {}, "Style"), h("div", { class: "ax-seg" }, [["full", "Explained, with figures"], ["recsys", "A paragraph"], ["brief", "Brief"], ["bullets", "Bullets"]].map(([k, t]) => h("button", { class: cfg.style === k ? "on" : "", onclick: (e) => { cfg.style = k; [...e.currentTarget.parentNode.children].forEach((b) => b.classList.remove("on")); e.currentTarget.classList.add("on"); } }, t)))),
    slider("Figures a paper (explained style)", 0, 3, 1, () => cfg.figures ?? 2, (v) => (cfg.figures = v), () => {}),
    slider("Sentences a paper (paragraph style)", 2, 12, 1, () => cfg.sentences, (v) => (cfg.sentences = v), () => {}),
    h("div", { class: "ax-f" }, h("label", {}, "Written for"), audIn),
    h("div", { class: "ax-f" }, h("label", {}, "Extras"), checks([["tools", "Tools"], ["benchmarks", "Benchmarks"], ["surveys", "Surveys"]], cfg.extras, changed)),
    h("div", { class: "ax-f" }, h("label", { class: "ax-check" }, h("label", { class: cfg.affiliations ? "on" : "", onclick: (e) => { e.preventDefault(); cfg.affiliations = !cfg.affiliations; e.currentTarget.classList.toggle("on"); } }, "Say who each paper is from (reads its arXiv page)"))),
    h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap" } }, saveBtn, writeBtn, feedId ? h("a", { class: "ax-btn", href: `feeds/${feedId}/rss.xml`, target: "_blank" }, "RSS") : null));
  fill(main, h("div", {},
    h("div", { class: "ax-ph" }, h("div", { class: "ey" }, h("a", { href: app.href("feeds") }, "Feeds"), "→", feedId ? "Studio" : "New feed"), h("h1", {}, "Feed studio"), h("div", { class: "au" }, "Every change re-picks the papers at once (no model is called for the preview). Writing an issue reads each paper's full text on arXiv, explains it, and copies the figures that explain it best; arXiv asks programs to read a page every 15 seconds, so an issue of 10 papers takes about ten minutes, and a paper read once is kept.")),
    h("div", { class: "ax-studio" }, form,
      h("div", {}, status,
        issues.length ? h("div", { class: "ax-card" }, h("h4", {}, "Issues"), h("div", { class: "ax-people" }, issues.map((i) => [h("a", { href: app.href("issue", i.id) }, i.title), h("span", { class: "n" }, ago(i.made))]))) : null,
        sec("Preview of the next issue"), preview))));
  runPreview();
}

// A figure's image, copied from the paper's page and kept: fetched as JSON
// when it scrolls near, and shown as an image (so an SVG never runs).
function figImage(app, m) {
  const ph = h("div", { class: "ph", style: m.w && m.h ? { aspectRatio: `${m.w} / ${m.h}`, minHeight: "0" } : {} });
  const load = async () => {
    try {
      const j = await app.ctx.api(`/v1/figure/${String(m.id).split("/").map(enc).join("/")}`);
      if (!j.data) throw new Error("no image");
      const img = h("img", { src: `data:${j.mime};base64,${j.data}`, alt: m.alt || "", loading: "lazy" });
      if (m.w && m.h) img.style.aspectRatio = `${m.w} / ${m.h}`;
      ph.replaceWith(img);
    } catch { ph.style.animation = "none"; ph.textContent = ""; }
  };
  if ("IntersectionObserver" in window) {
    const io = new IntersectionObserver((es) => { if (es.some((e) => e.isIntersecting)) { io.disconnect(); load(); } }, { rootMargin: "600px" });
    setTimeout(() => io.observe(ph), 0);
  } else load();
  return ph;
}

function figureBlock(app, f) {
  return h("figure", { class: "fig" },
    h("div", { class: "imgs" }, (f.images || []).map((m) => figImage(app, { ...m, alt: `${f.label}: ${f.caption || ""}` }))),
    h("figcaption", {}, h("b", {}, `${f.label}. `), f.why || f.caption || ""),
    f.why && f.caption ? h("details", {}, h("summary", {}, "The paper's caption"), h("div", { style: { marginTop: "4px" } }, f.caption, " ", h("a", { href: f.anchor, target: "_blank", rel: "noopener", style: { color: "var(--accent)" } }, "In the paper ↗"))) : null);
}

// Text whose [n] name papers of the issue, made links to them.
function issueRefs(text, ids) {
  const out = [];
  let last = 0;
  for (const m of String(text || "").matchAll(/\[(\d+)\]/g)) {
    const n = Number(m[1]);
    if (n < 1 || n > ids.length) continue;
    out.push(text.slice(last, m.index), h("a", { href: "javascript:void 0", onclick: () => document.getElementById(`ax-is-${n}`)?.scrollIntoView({ behavior: "smooth", block: "start" }) }, `[${n}]`));
    last = m.index + m[0].length;
  }
  out.push(String(text || "").slice(last));
  return out;
}

function paperEntry(app, p, i) {
  const link = (href, t) => h("a", { href, target: "_blank", rel: "noopener" }, t);
  const links = h("div", { class: "links" }, link(`https://arxiv.org/abs/${p.id}`, "📚 arXiv"), link(`https://arxiv.org/pdf/${p.id}`, "📄 PDF"), p.full_text ? link(`https://arxiv.org/html/${p.id}`, "🌐 HTML") : null, p.code ? link(p.code, "👨🏽‍💻 Code") : null, p.hf ? link(p.hf, "🤗 Hugging Face") : null);
  const who = [(p.orgs || []).slice(0, 4).join(" · "), (p.authors || "").split(",").slice(0, 4).join(",") + ((p.authors || "").split(",").length > 4 ? " et al." : "")].filter(Boolean).join(" — ");
  const head = [h("h2", {}, h("span", { class: "n" }, `${i + 1}.`), h("a", { href: app.href("paper", p.id) }, p.title)), who ? h("div", { class: "who" }, who) : null];
  if (!p.brief) return h("div", { class: "item", id: `ax-is-${i + 1}` }, head, h("p", {}, p.blurb), links);
  const sec = (label, text) => (text ? [h("span", { class: "lab" }, label), h("p", {}, text)] : null);
  const figs = p.figures || [];
  const t = p.table;
  return h("div", { class: "item", id: `ax-is-${i + 1}` }, head,
    h("div", { class: "brief" }, p.brief),
    figs[0] ? figureBlock(app, figs[0]) : null,
    sec("The problem", p.problem),
    sec("The approach", p.approach),
    sec("The results", p.results),
    figs.slice(1).map((f) => figureBlock(app, f)),
    t && (t.rows || []).length > 1 ? [h("span", { class: "lab" }, t.label), h("div", { class: "tbl" }, h("table", {}, t.rows.map((r) => h("tr", {}, r.map((c) => h("td", {}, c)))))), h("div", { class: "note" }, t.why || t.caption, " ", h("a", { href: t.anchor, target: "_blank", rel: "noopener", style: { color: "var(--accent)" } }, "In the paper ↗"))] : null,
    sec("Takeaway", p.takeaway),
    sec("Limits", p.limits),
    (p.terms || []).length ? [h("span", { class: "lab" }, "Terms"), h("dl", { class: "terms" }, p.terms.map((x) => [h("dt", {}, x.term), h("dd", {}, x.meaning)]))] : null,
    p.full_text === false ? h("div", { class: "note" }, "Explained from the abstract: arXiv has no HTML version of this paper to read or take figures from.") : null,
    links);
}

async function issuePage(app, main, id, alive) {
  fill(main, skel(12));
  const j = await app.api(`/v1/issue/${enc(id)}`);
  if (!alive()) return;
  const is = j.issue || {};
  const copy = h("button", { class: "ax-btn", onclick: async () => { try { await navigator.clipboard.writeText(is.markdown || ""); copy.textContent = "Copied ✓"; } catch { copy.textContent = "Copy failed"; } } }, "Copy as Markdown");
  const [name] = (j.title || "").split(" — ");
  const papers = is.papers || [];
  const full = papers.some((p) => p.brief);
  fill(main, h("div", { class: "ax-news" },
    h("div", { class: "ax-ph" }, h("div", { class: "ey" }, h("a", { href: app.href("feed", is.feed) }, "← Studio"), `made ${ago(j.made)}`, is.ms ? `in ${Math.round(is.ms / 1000)} s` : "", is.model ? `· ${is.model}` : "")),
    h("h1", {}, j.title),
    h("div", { class: "tag" }, `${papers.length} papers chosen from ${fmt(is.candidates)} for ${name}${full ? ", each explained from its full text with its figures" : ""}.`),
    h("div", { style: { display: "flex", gap: "8px", marginBottom: "14px" } }, copy, h("a", { class: "ax-btn", href: `feeds/${is.feed}/rss.xml`, target: "_blank" }, "RSS")),
    is.intro ? h("p", { class: "open" }, issueRefs(is.intro, papers)) : null,
    full && papers.length > 1 ? h("ol", { class: "toc" }, papers.map((p, i) => h("li", {}, h("a", { href: "javascript:void 0", onclick: () => document.getElementById(`ax-is-${i + 1}`)?.scrollIntoView({ behavior: "smooth", block: "start" }) }, p.title), p.brief ? h("span", {}, p.brief.split(/(?<=\.)\s/)[0]) : null))) : null,
    papers.map((p, i) => paperEntry(app, p, i)),
    [["tools", "Extras: Tools"], ["benchmarks", "Extras: Benchmarks"], ["surveys", "Extras: Surveys"]].map(([k, t]) => (is[k] || []).length ? h("div", { class: "item extra" }, h("h2", {}, t), h("ul", {}, is[k].map((x) => h("li", {}, h("a", { href: app.href("paper", x.id), style: { fontWeight: 600 } }, x.title), " — ", x.line)))) : null)));
}

// ----------------------------------------------------------------- about

function aboutPage(app, main) {
  fill(main, h("div", { class: "ax-about" },
    h("h1", {}, "How this works"),
    h("p", {}, "Every arXiv paper since 1991 is a row of one ChloeDB table, ax_papers: its title and abstract indexed for words (BM25, with typos and synonyms) and for meaning (a vector of each, made by the database's own embedding model on its CPU), its categories, authors, techniques, domains, venue, citations and topic as columns. People, citations, coauthors, topics and reading events are tables beside it. Everything on these pages is SQL over them; the SQL button shows the statements behind the page you are looking at."),
    h("h2", {}, "Search"),
    h("p", {}, "The box reads operators (", h("code", {}, "author:hinton cat:cs.LG since:2020 cites>100 has:code -survey"), ") and the same said in a sentence (", h("code", {}, "surveys of diffusion models since 2022 with code"), "), and it reads questions: who writes most on a topic, how many papers per year, is it growing, A vs B, where it began, where it is published, a reading list. Those are answered by SQL alone, with no model; a summary of the research is written by a model from the most relevant abstracts, each claim cited."),
    h("pre", {}, "-- candidates by words and meaning, then ranked together\nSELECT id FROM ax_papers ORDER BY bm25(title, :q, fuzzy => 'auto', synonyms => 1) * (1 + 0.25 * pop) DESC LIMIT 200;\nSELECT id FROM ax_papers ORDER BY similarity(emb, :v) DESC LIMIT 100;"),
    h("h2", {}, "What to read next"),
    h("p", {}, "ChloeDB's recommender counts reading events as they are written: what came after what, what was read together. It was taught by 1.4 million reference lists (each work that cites arXiv papers is a reader whose list is what it cites), and it learns from you: each paper you open is an event. A paper's Read next fuses what readers cite with it, the nearest by meaning, and personalized PageRank from it over the citation graph, all in one ORDER BY:"),
    h("pre", {}, "-- on the graph server: PageRank from the paper over the citations\nSELECT id, score() AS score FROM ax_nodes WHERE id NOT IN (:seeds) ORDER BY ppr(ax_cites, :seeds) DESC LIMIT 60;\n-- on the search server: those lists fused with the nearest by meaning\nSELECT id FROM ax_papers WHERE id != :id ORDER BY rrf(ranked(:cited_with), ranked(:walked), similarity(emb, :v)) DESC LIMIT 12;"),
    h("h2", {}, "Topics and the atlas"),
    h("p", {}, "Every paper's vector was clustered into 1,000 topics, each named from its most cited titles and placed in one of 17 fields (arXiv's areas); a new paper joins the topic whose centroid is nearest, found by the database's similarity search."),
    h("h2", {}, "Feeds"),
    h("p", {}, "A feed is a saved configuration: interests in your words, papers like some you name, categories, techniques, people, what to leave out, how many papers and how long. The papers are chosen by SQL (each interest's papers by words and meaning, fused). Each paper is then read in full from its HTML page on arxiv.org (its sections, figures and tables) and explained by a model through ChloeDB's llm(): in brief, the problem, the approach, the results with their numbers, the takeaway and the limits, with the figures that explain it best copied into the issue. Pages are read once and kept, fifteen seconds apart as arXiv's robots.txt asks.")));
}

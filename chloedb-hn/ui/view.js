// Hacker News on ChloeDB: the page, in ChloeDB's command center
// (#/app/hn, loaded through chloedb-server from this app) and on its own
// (the app's / with --public, through ui/index.html). It imports nothing
// and builds its own elements, so the same file serves both.
//
//   (home)              the box, what it understands, the front page now
//   search/<query>      stories and comments, facets, the query understood
//   item/<id>           a discussion, the comment found opened in its thread,
//                       and what is around it: the same link discussed
//                       again, links between discussions, nearest by meaning
//   user/<name>         a person: their best, their sites, their orbit in
//                       the reply graph
//   people/<topic>      who knows a topic, and how they talk to each other
//   trends/<terms>      how much HN talked about things, year by year
//   ask/<question>      what HN thinks, summed up with every claim cited
//   front               the front page as the tables rank it, live
//
// Every answer of the API carries the SQL it ran: the "SQL" button at the
// bottom shows the queries behind the page in view.

export default { render };

const CSS = `
.hn { --hn-w: 1180px; color: var(--text); font: 14px/1.5 var(--sans); max-width: var(--hn-w); margin: 0 auto; }
.hn * { box-sizing: border-box; }
.hn a { color: inherit; text-decoration: none; }
.hn a:hover { text-decoration: underline; text-underline-offset: 2px; }
.hn mark { background: color-mix(in srgb, var(--warn) 34%, transparent); color: inherit; border-radius: 3px; padding: 0 1px; }
.hn-top { position: sticky; top: 0; z-index: 5; background: color-mix(in srgb, var(--bg) 88%, transparent); backdrop-filter: blur(10px); padding: 12px 0 8px; }
.hn-bar { display: flex; align-items: center; gap: 14px; }
.hn-brand { display: flex; align-items: center; gap: 9px; font-weight: 650; letter-spacing: -.01em; white-space: nowrap; font-size: 15px; }
.hn-brand svg { width: 26px; height: 26px; color: var(--accent); }
.hn-brand small { font-weight: 500; color: var(--muted); font-size: 12px; }
.hn-box { position: relative; flex: 1; min-width: 0; }
.hn-box input { width: 100%; font: 15px var(--sans); padding: 10px 14px 10px 38px; border-radius: 11px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; box-shadow: var(--shadow-1); }
.hn-box input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.hn-box > svg { position: absolute; left: 12px; top: 50%; width: 17px; height: 17px; transform: translateY(-50%); color: var(--muted); pointer-events: none; }
.hn-fast { flex: none; display: inline-flex; align-items: center; gap: 7px; font: 600 13px var(--sans); padding: 9px 13px; border-radius: 11px; border: 1px solid var(--line-2); background: var(--surface); color: var(--muted); cursor: pointer; box-shadow: var(--shadow-1); }
.hn-fast .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--line-2); }
.hn-fast.on { color: var(--accent); border-color: var(--accent); background: var(--accent-soft); }
.hn-fast.on .dot { background: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.hn-box .kbd { position: absolute; right: 10px; top: 50%; transform: translateY(-50%); font: 11px var(--mono); color: var(--muted); border: 1px solid var(--line-2); border-radius: 5px; padding: 1px 6px; }
.hn-sugg { position: absolute; left: 0; right: 0; top: calc(100% + 6px); background: var(--surface); border: 1px solid var(--line-2); border-radius: 11px; box-shadow: var(--shadow-2); padding: 6px; display: none; z-index: 9; }
.hn-sugg.on { display: block; }
.hn-sugg .it { display: flex; gap: 10px; align-items: baseline; padding: 7px 9px; border-radius: 7px; cursor: pointer; }
.hn-sugg .it.sel, .hn-sugg .it:hover { background: var(--accent-soft); }
.hn-sugg .it .k { font-size: 11px; color: var(--muted); width: 54px; flex: none; text-transform: uppercase; letter-spacing: .04em; }
.hn-sugg .it .m { margin-left: auto; color: var(--muted); font-size: 12px; white-space: nowrap; }
.hn-nav { display: flex; gap: 2px; margin-top: 8px; overflow-x: auto; scrollbar-width: none; }
.hn-nav a { padding: 5px 11px; border-radius: 8px; color: var(--text-2); font-size: 13px; white-space: nowrap; }
.hn-nav a:hover { background: var(--surface-2); text-decoration: none; }
.hn-nav a.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.hn-nav .sp { flex: 1; }
.hn-nav .live { display: inline-flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; padding: 5px 4px; white-space: nowrap; }
.hn-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--good); box-shadow: 0 0 0 0 color-mix(in srgb, var(--good) 60%, transparent); animation: hnpulse 2s infinite; }
@keyframes hnpulse { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--good) 55%, transparent); } 70% { box-shadow: 0 0 0 7px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
.hn-main { padding: 6px 0 70px; min-height: 60vh; }
.hn-und { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin: 4px 0 12px; font-size: 12.5px; color: var(--muted); }
.hn-chip { display: inline-flex; align-items: center; gap: 5px; padding: 3px 9px; border-radius: 999px; background: var(--surface); border: 1px solid var(--line-2); color: var(--text-2); font-size: 12.5px; white-space: nowrap; cursor: pointer; }
.hn-chip:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.hn-chip.f { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.hn-chip .x { opacity: .6; font-size: 13px; line-height: 1; }
.hn-chip .from { color: var(--muted); font-family: var(--mono); font-size: 11px; }
.hn-grid { display: grid; grid-template-columns: 216px minmax(0, 1fr) 268px; gap: 26px; align-items: start; }
.hn-grid.two { grid-template-columns: minmax(0, 1fr) 300px; }
@media (max-width: 1100px) { .hn-grid { grid-template-columns: 200px minmax(0, 1fr); } .hn-grid > .hn-right { display: none; } }
@media (max-width: 760px) { .hn-grid, .hn-grid.two { grid-template-columns: minmax(0, 1fr); } .hn-grid > .hn-left { display: none; } .hn-brand small { display: none; } }
.hn-left { position: sticky; top: 108px; max-height: calc(100vh - 120px); overflow: auto; scrollbar-width: thin; padding-right: 4px; }
.hn-facet { margin-bottom: 18px; }
.hn-facet h4 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; }
.hn-facet .r { display: flex; align-items: center; gap: 8px; padding: 3px 6px; margin: 0 -6px; border-radius: 6px; cursor: pointer; font-size: 13px; color: var(--text-2); }
.hn-facet .r:hover { background: var(--surface-2); color: var(--text); }
.hn-facet .r.on { color: var(--accent); font-weight: 600; }
.hn-facet .r .n { margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; }
.hn-facet .r .bar { position: relative; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.hn-years { display: flex; align-items: flex-end; gap: 2px; height: 54px; margin: 4px 0 2px; }
.hn-years div { flex: 1; background: color-mix(in srgb, var(--accent) 55%, transparent); border-radius: 2px 2px 0 0; min-height: 2px; cursor: pointer; position: relative; }
.hn-years div:hover { background: var(--accent); }
.hn-years div.on { background: var(--accent); }
.hn-yl { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--muted); }
.hn-head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 4px; }
.hn-head .t { font-size: 13px; color: var(--muted); }
.hn-head .t b { color: var(--text); font-weight: 600; }
.hn-seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 8px; overflow: hidden; background: var(--surface); }
.hn-seg button { font: 12.5px var(--sans); border: 0; background: none; color: var(--text-2); padding: 4px 10px; cursor: pointer; }
.hn-seg button + button { border-left: 1px solid var(--line); }
.hn-seg button.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.hn-head .sp { flex: 1; }
.hn-dym { font-size: 14px; margin: 2px 0 10px; color: var(--text-2); }
.hn-dym a { color: var(--accent); font-weight: 600; font-style: italic; }
.hn-res { list-style: none; margin: 0; padding: 0; }
.hn-s { display: grid; grid-template-columns: 46px minmax(0, 1fr); gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--line); }
.hn-s:last-child { border-bottom: 0; }
.hn-pts { text-align: center; padding-top: 2px; }
.hn-pts b { display: block; font-size: 15px; font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
.hn-pts span { font-size: 10px; color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }
.hn-ttl { font-size: 15.5px; font-weight: 560; line-height: 1.35; letter-spacing: -.005em; }
.hn-ttl a { color: var(--text); }
.hn-ttl .site { font-size: 12.5px; color: var(--muted); font-weight: 400; margin-left: 6px; }
.hn-meta { display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 12.5px; color: var(--muted); margin-top: 3px; align-items: center; }
.hn-meta a:hover { color: var(--accent); }
.hn-meta .k { font-size: 11px; font-weight: 600; padding: 0 6px; border-radius: 5px; background: var(--surface-3); color: var(--text-2); text-transform: uppercase; letter-spacing: .03em; }
.hn-meta .k.show { background: var(--good-soft); color: var(--good-ink); }
.hn-meta .k.ask { background: var(--accent-soft); color: var(--accent); }
.hn-meta .k.launch { background: var(--serious-soft); color: var(--s2); }
.hn-meta .tp { color: var(--text-2); }
.hn-meta .tp:before { content: "#"; opacity: .5; }
.hn-snip { font-size: 13.5px; color: var(--text-2); margin-top: 5px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.hn-again { font-size: 12.5px; margin-top: 5px; color: var(--muted); }
.hn-again a { color: var(--accent); }
.hn-c { padding: 12px 0 12px 14px; border-left: 2px solid var(--line-2); margin: 10px 0; }
.hn-c:hover { border-left-color: var(--accent); }
.hn-c .on { font-size: 12.5px; color: var(--muted); }
.hn-c .on a { color: var(--text); font-weight: 560; }
.hn-c .txt { font-size: 14px; margin-top: 5px; white-space: pre-wrap; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 5; -webkit-box-orient: vertical; overflow: hidden; }
.hn-c .txt.open { display: block; }
.hn-sec { display: flex; align-items: baseline; justify-content: space-between; margin: 22px 0 4px; }
.hn-sec h3 { margin: 0; font-size: 13px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; }
.hn-sec a { font-size: 13px; color: var(--accent); }
.hn-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; box-shadow: var(--shadow-1); }
.hn-card h4 { margin: 0 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; display: flex; justify-content: space-between; }
.hn-card h4 a { text-transform: none; letter-spacing: 0; color: var(--accent); font-weight: 500; }
.hn-ans { border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--line)); background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 6%, var(--surface)), var(--surface)); border-radius: var(--r); padding: 14px 16px 12px; margin: 10px 0 14px; }
.hn-ans .ey { font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--accent); font-weight: 650; display: flex; gap: 8px; align-items: center; }
.hn-ans .ey .nm { color: var(--muted); font-weight: 500; text-transform: none; letter-spacing: 0; margin-left: auto; cursor: pointer; }
.hn-ans .ey .nm:hover { color: var(--accent); }
.hn-ans .q { font-size: 14px; color: var(--text-2); margin: 4px 0 8px; }
.hn-ans .big { font-size: 19px; font-weight: 680; line-height: 1.3; letter-spacing: -.01em; }
.hn-ans .big a { color: var(--text); }
.hn-ans .big a:hover { color: var(--accent); }
.hn-ans .num { font-size: 34px; font-weight: 720; letter-spacing: -.02em; line-height: 1.1; }
.hn-ans .meta { font-size: 13px; color: var(--muted); margin-top: 4px; display: flex; flex-wrap: wrap; gap: 4px 12px; }
.hn-ans .meta b { color: var(--text); font-weight: 650; }
.hn-ans .txt { font-size: 14px; margin-top: 6px; white-space: pre-wrap; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 6; -webkit-box-orient: vertical; overflow: hidden; }
.hn-ans ol { margin: 6px 0 0; padding: 0; list-style: none; counter-reset: r; }
.hn-ans ol li { counter-increment: r; display: grid; grid-template-columns: 22px 1fr auto; gap: 8px; align-items: center; padding: 5px 0; border-top: 1px solid var(--line); font-size: 13.5px; }
.hn-ans ol li:first-child { border-top: 0; }
.hn-ans ol li:before { content: counter(r); color: var(--muted); font-size: 12px; text-align: right; }
.hn-ans ol li .n { color: var(--muted); font-size: 12.5px; font-variant-numeric: tabular-nums; white-space: nowrap; }
.hn-ans ol li .who { display: flex; align-items: center; gap: 7px; min-width: 0; }
.hn-ans ol li .who a { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.hn-ans .yrs { display: flex; align-items: flex-end; gap: 3px; height: 64px; margin-top: 10px; }
.hn-ans .yrs div { flex: 1; background: color-mix(in srgb, var(--accent) 35%, transparent); border-radius: 2px 2px 0 0; min-height: 2px; position: relative; }
.hn-ans .yrs div.pk { background: var(--accent); }
.hn-ans .yl { display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); margin-top: 3px; }
.hn-ans .note { font-size: 12px; color: var(--muted); margin-top: 6px; }
.hn-sugg .hd { font-size: 10.5px; text-transform: uppercase; letter-spacing: .07em; color: var(--muted); padding: 7px 9px 2px; }
.hn-sugg .it .q { flex: none; width: 18px; height: 18px; border-radius: 5px; display: inline-grid; place-items: center; background: var(--accent-soft); color: var(--accent); font-size: 11px; font-weight: 700; }
.hn-sugg .it .t b { font-weight: 650; }
.hn-sugg .it .t { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.hn-mini { list-style: none; margin: 0; padding: 0; }
.hn-mini li { padding: 6px 0; border-top: 1px solid var(--line); font-size: 13px; line-height: 1.35; }
.hn-mini li:first-child { border-top: 0; padding-top: 0; }
.hn-mini .m { display: block; font-size: 11.5px; color: var(--muted); margin-top: 2px; }
.hn-ppl { display: flex; flex-wrap: wrap; gap: 6px; }
.hn-ppl a { display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; padding: 3px 8px 3px 3px; border-radius: 999px; background: var(--surface-2); }
.hn-ava { width: 20px; height: 20px; border-radius: 50%; display: inline-grid; place-items: center; font-size: 10px; font-weight: 700; color: #fff; flex: none; text-transform: uppercase; }
.hn-ask { display: flex; gap: 10px; align-items: center; padding: 12px 14px; border-radius: var(--r); background: linear-gradient(135deg, var(--accent-soft), color-mix(in srgb, var(--s7) 14%, var(--surface))); border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent); cursor: pointer; margin-bottom: 14px; }
.hn-ask:hover { border-color: var(--accent); text-decoration: none !important; }
.hn-ask svg { width: 22px; height: 22px; color: var(--accent); flex: none; }
.hn-ask b { display: block; font-size: 13.5px; }
.hn-ask span { font-size: 12px; color: var(--text-2); }
.hn-empty { padding: 40px 0; text-align: center; color: var(--muted); }
.hn-trend { list-style: none; margin: 6px 0 0; padding: 0; }
.hn-trend li { display: flex; gap: 12px; padding: 9px 2px; border-top: 1px solid var(--line); }
.hn-trend li:first-child { border-top: 0; }
.hn-trend .rk { width: 22px; flex: none; text-align: right; color: var(--muted); font-variant-numeric: tabular-nums; font-size: 13px; padding-top: 2px; }
.hn-trend .tm { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; }
.hn-trend .tm > a:first-child { font-weight: 680; font-size: 15.5px; color: var(--text); }
.hn-trend .x { font-size: 12px; font-weight: 650; color: var(--accent); background: var(--accent-soft); border-radius: 6px; padding: 1px 7px; font-variant-numeric: tabular-nums; }
.hn-trend .m { font-size: 12.5px; color: var(--muted); }
.hn-trend .st { display: block; font-size: 13px; color: var(--text-2); margin-top: 3px; }
.hn-sub { font-size: 12.5px; color: var(--muted); margin: 2px 0 8px; max-width: 820px; }
.hn-months { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 12px; margin-top: 10px; }
.hn-months .lead { display: block; font-size: 13px; color: var(--text-2); margin: 2px 0 8px; }
.hn-months .chips { display: flex; flex-wrap: wrap; gap: 6px; }
.hn-months .hn-chip small { color: var(--accent); font-weight: 650; }
.hn-think { display: flex; align-items: center; gap: 12px; padding: 4px 0; }
.hn-think .orb { width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; flex: none; color: var(--accent); background: radial-gradient(circle at 35% 35%, color-mix(in srgb, var(--accent) 30%, transparent), transparent 70%); animation: hnorb 2.4s ease-in-out infinite; }
.hn-think .orb svg { width: 22px; height: 22px; animation: hnspin 6s linear infinite; }
.hn-think b { display: block; font-size: 14.5px; background: linear-gradient(90deg, var(--text), var(--accent), var(--text)); background-size: 200% 100%; -webkit-background-clip: text; background-clip: text; color: transparent; animation: hnsk 2s linear infinite; }
.hn-think .st { font-size: 12.5px; color: var(--muted); }
.hn-think .dots { margin-left: auto; display: flex; gap: 4px; }
.hn-think .dots i { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); opacity: .3; animation: hndot 1.2s infinite; }
.hn-think .dots i:nth-child(2) { animation-delay: .2s; }
.hn-think .dots i:nth-child(3) { animation-delay: .4s; }
.hn-in { animation: hnfade .5s ease-out; }
@keyframes hnorb { 0%, 100% { transform: scale(1); box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 35%, transparent); } 50% { transform: scale(1.06); box-shadow: 0 0 0 8px transparent; } }
@keyframes hnspin { to { transform: rotate(360deg); } }
@keyframes hndot { 0%, 100% { opacity: .25; transform: translateY(0); } 40% { opacity: 1; transform: translateY(-3px); } }
@keyframes hnfade { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
.hn-skel { height: 64px; border-radius: 8px; margin: 10px 0; background: linear-gradient(90deg, var(--surface-2), var(--surface-3), var(--surface-2)); background-size: 200% 100%; animation: hnsk 1.2s infinite; }
@keyframes hnsk { 0% { background-position: 100% 0; } 100% { background-position: -100% 0; } }
.hn-more { display: block; width: 100%; margin: 14px 0; padding: 9px; border-radius: 9px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); font: 13px var(--sans); cursor: pointer; }
.hn-more:hover { border-color: var(--accent); color: var(--accent); }
.hn-hero { text-align: center; padding: 56px 0 26px; }
.hn-hero h1 { font-size: 38px; line-height: 1.12; letter-spacing: -.03em; margin: 0 0 10px; font-weight: 700; }
.hn-hero h1 em { font-style: normal; background: linear-gradient(90deg, var(--accent), var(--s7)); -webkit-background-clip: text; background-clip: text; color: transparent; }
.hn-hero p { color: var(--text-2); font-size: 15.5px; margin: 0 auto 22px; max-width: 640px; }
.hn-try { display: flex; flex-wrap: wrap; justify-content: center; gap: 8px; max-width: 860px; margin: 0 auto; }
.hn-try a { display: inline-flex; flex-direction: column; align-items: flex-start; text-align: left; gap: 1px; padding: 8px 12px; border-radius: 10px; border: 1px solid var(--line-2); background: var(--surface); font-size: 13.5px; box-shadow: var(--shadow-1); }
.hn-try a:hover { border-color: var(--accent); text-decoration: none; }
.hn-try a small { font-size: 11px; color: var(--muted); }
.hn-try a code { font: 12.5px var(--mono); color: var(--text); }
.hn-stats { display: flex; justify-content: center; gap: 34px; margin: 30px 0 8px; flex-wrap: wrap; }
.hn-stats div { text-align: center; }
.hn-stats b { display: block; font-size: 22px; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.hn-stats span { font-size: 12px; color: var(--muted); }
.hn-cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 16px; margin-top: 26px; }
.hn-story { margin-bottom: 12px; }
.hn-story h1 { font-size: 22px; letter-spacing: -.015em; line-height: 1.25; margin: 0 0 6px; }
.hn-story .body { font-size: 14.5px; white-space: pre-wrap; margin-top: 10px; overflow-wrap: anywhere; }
.hn-tree { margin-top: 14px; }
.hn-cm { position: relative; padding: 8px 0 2px; }
.hn-cm .hd { font-size: 12.5px; color: var(--muted); display: flex; gap: 8px; align-items: center; }
.hn-cm .hd a.u { color: var(--text-2); font-weight: 600; }
.hn-cm .hd .tg { cursor: pointer; font-family: var(--mono); font-size: 11px; color: var(--muted); user-select: none; }
.hn-cm .bd { font-size: 14px; white-space: pre-wrap; overflow-wrap: anywhere; margin: 3px 0 2px; }
.hn-cm .kids { margin-left: 18px; padding-left: 12px; border-left: 1px solid var(--line); }
.hn-cm.shut > .bd, .hn-cm.shut > .kids { display: none; }
.hn-cm.focus > .bd { background: color-mix(in srgb, var(--warn) 16%, transparent); border-radius: 6px; padding: 6px 8px; margin-left: -8px; box-shadow: inset 3px 0 0 var(--warn); }
.hn-cm.path > .hd a.u { color: var(--accent); }
.hn-pf { display: flex; gap: 16px; align-items: center; margin-bottom: 14px; }
.hn-pf .hn-ava { width: 56px; height: 56px; font-size: 22px; }
.hn-pf h1 { margin: 0; font-size: 24px; letter-spacing: -.02em; }
.hn-kpis { display: flex; gap: 22px; flex-wrap: wrap; font-size: 12.5px; color: var(--muted); margin-top: 4px; }
.hn-kpis b { color: var(--text); font-size: 15px; margin-right: 4px; font-variant-numeric: tabular-nums; }
.hn-graph { width: 100%; height: 380px; display: block; }
.hn-graph text { font: 11.5px var(--sans); fill: var(--text-2); pointer-events: none; }
.hn-graph .nd { cursor: pointer; }
.hn-graph .nd:hover circle { stroke: var(--accent); stroke-width: 2.5; }
.hn-graph line { stroke: var(--line-2); }
.hn-chart { width: 100%; height: 300px; display: block; }
.hn-chart text { font: 11px var(--sans); fill: var(--muted); }
.hn-legend { display: flex; flex-wrap: wrap; gap: 14px; font-size: 13px; margin: 6px 0 12px; }
.hn-legend span { display: inline-flex; align-items: center; gap: 6px; }
.hn-legend i { width: 12px; height: 3px; border-radius: 2px; display: inline-block; }
.hn-ans { font-size: 15px; line-height: 1.6; }
.hn-ans p { margin: 0 0 10px; }
.hn-ans ul { margin: 0 0 6px; padding-left: 20px; }
.hn-ans li { margin: 6px 0; }
.hn-ans sup a { display: inline-block; min-width: 17px; text-align: center; font-size: 10.5px; font-weight: 700; color: var(--accent); background: var(--accent-soft); border-radius: 4px; padding: 0 3px; margin: 0 1px; line-height: 15px; }
.hn-src { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
.hn-src > div { border: 1px solid var(--line); border-radius: 9px; padding: 9px 11px; font-size: 12.5px; background: var(--surface); }
.hn-src > div.hot { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.hn-src .n { font-weight: 700; color: var(--accent); margin-right: 6px; }
.hn-src .x { color: var(--text-2); margin-top: 4px; display: -webkit-box; -webkit-line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; }
.hn-receipt { font-size: 12px; color: var(--muted); display: flex; gap: 14px; flex-wrap: wrap; margin: 8px 0 14px; }
.hn-input { display: flex; gap: 8px; margin: 6px 0 14px; }
.hn-input input { flex: 1; font: 15px var(--sans); padding: 9px 12px; border-radius: 10px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; }
.hn-input input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.hn-btn { font: 600 13px var(--sans); padding: 8px 14px; border-radius: 9px; border: 0; background: var(--accent); color: var(--accent-ink); cursor: pointer; white-space: nowrap; }
.hn-btn.ghost { background: var(--surface); color: var(--text-2); border: 1px solid var(--line-2); font-weight: 500; }
.hn-sql { position: fixed; right: 18px; bottom: 16px; z-index: 20; }
.hn-sql > button { font: 12px var(--mono); padding: 6px 11px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); cursor: pointer; box-shadow: var(--shadow-2); }
.hn-sql > button b { color: var(--accent); }
.hn-sqlp { position: fixed; right: 18px; bottom: 56px; width: min(720px, calc(100vw - 36px)); max-height: 70vh; overflow: auto; background: var(--surface); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: var(--shadow-3); padding: 12px 14px; z-index: 21; display: none; }
.hn-sqlp.on { display: block; }
.hn-sqlp .q { border-top: 1px solid var(--line); padding: 9px 0; }
.hn-sqlp .q:first-of-type { border-top: 0; }
.hn-sqlp .w { display: flex; justify-content: space-between; font-size: 12.5px; font-weight: 600; }
.hn-sqlp .w span { color: var(--muted); font-weight: 400; font-variant-numeric: tabular-nums; margin-left: auto; padding-left: 10px; }
.hn-sqlp .w .sv { font-size: 11px; color: var(--muted); font-weight: 400; }
.hn-sqlp .w .pt { font-style: normal; font-size: 10.5px; font-weight: 600; text-transform: uppercase; letter-spacing: .05em; color: var(--accent); background: var(--accent-soft); border-radius: 5px; padding: 1px 6px; margin-right: 7px; }
.hn-sqlp pre { margin: 5px 0 0; font: 11.5px/1.5 var(--mono); white-space: pre-wrap; background: var(--code); border-radius: 7px; padding: 7px 9px; color: var(--text-2); }
.hn-sqlp .bad { color: var(--crit-ink); font-size: 12px; }
.hn-foot { font-size: 12px; color: var(--muted); text-align: center; padding: 26px 0 6px; border-top: 1px solid var(--line); margin-top: 30px; }
.hn-foot a { color: var(--text-2); }
.hn-err { padding: 12px 14px; border-radius: 9px; background: var(--crit-soft); color: var(--crit-ink); font-size: 13.5px; margin: 10px 0; }
.hn-link-h { display: flex; gap: 10px; align-items: baseline; flex-wrap: wrap; margin: 4px 0 2px; }
.hn-link-h h1 { font: 650 21px/1.25 var(--sans); margin: 0; word-break: break-all; }
.hn-link-h h1 a { color: var(--text); text-decoration: none; }
.hn-link-box { display: flex; gap: 8px; margin: 6px 0 14px; }
.hn-link-box input { flex: 1; min-width: 0; font: 14px var(--sans); padding: 9px 12px; border-radius: 10px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); }
.hn-link-box button { font: 600 13px var(--sans); padding: 9px 14px; border-radius: 10px; border: 1px solid var(--accent); background: var(--accent); color: #fff; cursor: pointer; }
.hn-lg { position: relative; background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); box-shadow: var(--shadow-1); margin-bottom: 14px; overflow: hidden; }
.hn-lg svg { display: block; width: 100%; height: auto; touch-action: none; }
.hn-lg .e { stroke: var(--line-2); stroke-width: 1.2; }
.hn-lg .e.cited { stroke: #5b8def; stroke-dasharray: 4 3; }
.hn-lg .e.links { stroke: var(--muted); stroke-dasharray: 2 3; }
.hn-lg .n { cursor: pointer; }
.hn-lg .n circle { stroke: var(--surface); stroke-width: 2; }
.hn-lg .n text { font: 11px var(--sans); fill: var(--text-2); pointer-events: none; paint-order: stroke; stroke: var(--surface); stroke-width: 3px; }
.hn-lg .n.link circle { fill: var(--accent); }
.hn-lg .n.story circle { fill: #f08a24; }
.hn-lg .n.cites circle { fill: #5b8def; }
.hn-lg .n.linked circle { fill: #9aa3ad; }
.hn-lg .n.user circle { fill: #2fa37a; }
.hn-lg.focus .n, .hn-lg.focus .e { opacity: .15; transition: opacity .12s; }
.hn-lg.focus .n.hot, .hn-lg.focus .e.hot { opacity: 1; }
.hn-lg .tip { position: absolute; pointer-events: none; max-width: 300px; background: var(--surface); border: 1px solid var(--line-2); border-radius: 9px; padding: 7px 10px; font-size: 12.5px; line-height: 1.35; box-shadow: var(--shadow-2); display: none; z-index: 2; }
.hn-lg .tip .m { display: block; color: var(--muted); font-size: 11.5px; margin-top: 2px; }
.hn-lg .legend { display: flex; flex-wrap: wrap; gap: 12px; padding: 8px 14px; border-top: 1px solid var(--line); font-size: 12px; color: var(--muted); }
.hn-lg .legend i { display: inline-block; width: 9px; height: 9px; border-radius: 50%; margin-right: 5px; vertical-align: -1px; }
.hn-roles { display: inline-flex; gap: 5px; margin-left: 6px; }
.hn-roles span { font-size: 10.5px; padding: 1px 6px; border-radius: 99px; background: var(--surface-2); color: var(--text-2); }
.hn-roles span.s { background: color-mix(in srgb, #f08a24 16%, transparent); }
.hn-roles span.c { background: color-mix(in srgb, #5b8def 16%, transparent); }
.hn-roles span.t { background: color-mix(in srgb, #2fa37a 16%, transparent); }
.hn-cite { padding: 9px 0; border-top: 1px solid var(--line); font-size: 13px; line-height: 1.45; }
.hn-cite:first-child { border-top: 0; padding-top: 2px; }
.hn-cite .on { font-size: 12px; color: var(--muted); margin-bottom: 3px; }
.hn-cite .txt { color: var(--text-2); overflow-wrap: anywhere; }
.hn-xlink { font-size: 12px; color: var(--accent); text-decoration: none; }
.hn-h3 { font: 650 14px var(--sans); margin: 4px 0 8px; color: var(--text-2); }
.hn-dg { max-width: 860px; margin: 0 auto; }
.hn-dg-top { display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap; margin: 6px 0 4px; }
.hn-dg-top h1 { font: 700 26px/1.2 var(--sans); margin: 0; letter-spacing: -.01em; }
.hn-dg-top .wk { color: var(--muted); font-size: 14px; }
.hn-dg-top .nv { margin-left: auto; display: flex; gap: 8px; font-size: 13px; }
.hn-dg-top .nv a { color: var(--accent); text-decoration: none; }
.hn-dg-intro { font: 16.5px/1.6 var(--sans); color: var(--text); margin: 10px 0 14px; padding: 14px 18px; border-left: 3px solid var(--accent); background: var(--accent-soft); border-radius: 0 10px 10px 0; }
.hn-dg-toc { columns: 2 320px; gap: 26px; margin: 8px 0 18px; padding: 0; list-style: none; counter-reset: toc; }
.hn-dg-toc li { counter-increment: toc; break-inside: avoid; padding: 4px 0; font-size: 13.5px; }
.hn-dg-toc li::before { content: counter(toc) ". "; color: var(--muted); }
.hn-dg-toc a { color: var(--text); text-decoration: none; }
.hn-dg-e { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 18px 20px; margin-bottom: 16px; box-shadow: var(--shadow-1); }
.hn-dg-e .n { font: 700 12px var(--mono); color: var(--accent); }
.hn-dg-e h2 { font: 650 19px/1.3 var(--sans); margin: 2px 0 4px; }
.hn-dg-e h2 a { color: var(--text); text-decoration: none; }
.hn-dg-e .img { float: right; width: 220px; max-height: 150px; object-fit: cover; border-radius: 9px; margin: 2px 0 10px 16px; border: 1px solid var(--line); }
.hn-dg-e h5 { font: 650 11px var(--sans); text-transform: uppercase; letter-spacing: .06em; color: var(--muted); margin: 12px 0 3px; }
.hn-dg-e p { margin: 0; font-size: 14.5px; line-height: 1.55; color: var(--text); }
.hn-dg-e .terms { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; clear: both; }
.hn-dg-e .terms span { font-size: 12px; padding: 3px 8px; border-radius: 7px; background: var(--surface-2); color: var(--text-2); }
.hn-dg-e .qs { clear: both; margin-top: 10px; border-top: 1px solid var(--line); padding-top: 8px; }
.hn-dg-e .q { font-size: 13px; line-height: 1.45; color: var(--text-2); padding: 6px 0; }
.hn-dg-e .q b { color: var(--text); font-weight: 600; }
.hn-dg-e .wait { color: var(--muted); font-size: 13.5px; font-style: italic; }
@media (max-width: 760px) { .hn-dg-e .img { float: none; width: 100%; max-height: 180px; margin: 6px 0; } }
`;

const ICON = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v8a2.5 2.5 0 0 1-2.5 2.5H11l-4.5 4v-4h0A2.5 2.5 0 0 1 4 13.5z"/><circle cx="11.5" cy="9.3" r="2.8"/><path d="M13.6 11.4 15.6 13.4"/></svg>`;
const SEARCH = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>`;
const SPARK = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1"/><circle cx="12" cy="12" r="3"/></svg>`;

// ---- small helpers ---------------------------------------------------------

function h(tag, attrs, ...kids) {
  const el = tag === "svg" || tag === "g" || tag === "line" || tag === "circle" || tag === "path" || tag === "text" || tag === "rect" || tag === "polyline"
    ? document.createElementNS("http://www.w3.org/2000/svg", tag) : document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "class") el.setAttribute("class", v);
    else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
    else if (k === "html") el.innerHTML = v;
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? "" : v);
  }
  add(el, kids);
  return el;
}
// Replaces an element's children, leaving out nulls and flattening lists.
function fill(el, ...kids) {
  el.replaceChildren();
  return add(el, kids);
}
function add(el, kids) {
  for (const k of kids.flat(9)) {
    if (k == null || k === false) continue;
    el.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
  return el;
}
const fmt = (n) => (n == null || isNaN(n) ? "–" : Number(n).toLocaleString("en-US"));
const compact = (n) => (n >= 1e6 ? (n / 1e6).toFixed(n >= 1e7 ? 0 : 1) + "M" : n >= 1e4 ? Math.round(n / 1e3) + "k" : n >= 1e3 ? (n / 1e3).toFixed(1) + "k" : String(n ?? 0));
function ago(iso) {
  const t = Date.parse(iso);
  if (!t) return "";
  const s = (Date.now() - t) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 30) return `${Math.round(s / 86400)} days ago`;
  if (s < 86400 * 365) return `${Math.round(s / 86400 / 30.4)} months ago`;
  const y = s / 86400 / 365.25;
  return y < 1.5 ? "a year ago" : `${Math.round(y)} years ago`;
}
const day = (iso) => { const t = new Date(iso); return isNaN(t) ? "" : t.toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" }); };
const COLORS = ["#2a78d6", "#eb6834", "#1baf7a", "#c98500", "#d55181", "#6250d6", "#0f8a8a", "#e34948"];
function hue(name) { let x = 0; for (const c of String(name)) x = (x * 31 + c.charCodeAt(0)) >>> 0; return COLORS[x % COLORS.length]; }
const ava = (name, size) => h("span", { class: "hn-ava", style: { background: hue(name), ...(size ? { width: size + "px", height: size + "px" } : {}) } }, String(name || "?").slice(0, 1));
const enc = encodeURIComponent;
const ESC = /[.*+?^${}()|[\]\\]/g;

// The words to mark in a result: the query's words and phrases, each as
// its stem (so "databases" marks "database"), at the start of a word.
function markers(und) {
  const words = [];
  for (const p of und?.phrases || []) words.push(p);
  for (const e of und?.exact || []) words.push(e);
  for (const w of String(und?.text || "").split(/\s+/)) {
    const x = w.replace(/^[-"'(]+|["'),.:;!?]+$/g, "");
    if (x.length < 2 || /^(the|and|for|with|how|what|why|are|you|does|about|of|in|on|to|is|a|an|vs)$/i.test(x)) continue;
    words.push(x);
  }
  if (!words.length) return null;
  const parts = words.map((w) => {
    let s = w.toLowerCase();
    if (/\s/.test(s)) return s.replace(ESC, "\\$&").replace(/\s+/g, "\\s+");
    if (s.length > 5) s = s.replace(/(ing|ed|es|s|ly|er)$/, "");
    return s.replace(ESC, "\\$&") + (/\w$/.test(s) ? "\\w*" : "");
  });
  try { return new RegExp(`(^|[^\\p{L}\\p{N}])(${parts.join("|")})`, "giu"); } catch { return null; }
}
function marked(text, re) {
  text = String(text ?? "");
  if (!re) return text;
  const out = [];
  let last = 0;
  re.lastIndex = 0;
  for (const m of text.matchAll(re)) {
    const at = m.index + m[1].length;
    if (at > last) out.push(text.slice(last, at));
    out.push(h("mark", {}, m[2]));
    last = at + m[2].length;
  }
  out.push(text.slice(last));
  return out;
}
// The stretch of a long text where the marked words are thickest.
function excerpt(text, re, n = 320) {
  text = String(text || "").replace(/\s+/g, " ").trim();
  if (text.length <= n || !re) return text.length <= n ? text : text.slice(0, n) + "…";
  re.lastIndex = 0;
  const hits = [...text.matchAll(re)].map((m) => m.index);
  if (!hits.length) return text.slice(0, n) + "…";
  let best = 0, bestN = 0;
  for (let i = 0; i < hits.length; i++) {
    let j = i; while (j < hits.length && hits[j] - hits[i] < n * 0.8) j++;
    if (j - i > bestN) { bestN = j - i; best = hits[i]; }
  }
  const from = Math.max(0, best - 60);
  const cut = text.slice(from, from + n);
  return (from > 0 ? "…" : "") + cut.replace(/^\S*\s/, from > 0 ? "" : "$&") + (from + n < text.length ? "…" : "");
}

// ---- the page --------------------------------------------------------------

function render(el, ctx) {
  if (!document.getElementById("hn-css")) document.head.append(h("style", { id: "hn-css" }, CSS));
  // Fast mode: every search by BM25 alone, no candidates by meaning and
  // no reranking, until switched off (kept in this browser).
  let fastOn = false;
  try { fastOn = localStorage.getItem("hn.fast") === "1"; } catch { /* storage off */ }
  const app = {
    ctx,
    get fast() { return fastOn; },
    queries: [],
    took: 0,
    seq: 0,
    status: null,
    api: async (path, opts) => {
      const j = await ctx.api(path, opts);
      if (j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    // The queries of a page's later requests (comments, facets, an
    // answer), added to the drawer under what they were for.
    add: (j, part) => {
      if (j && Array.isArray(j.queries)) { app.queries = app.queries.concat(j.queries.map((q) => ({ ...q, part }))); drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...slots(parts)),
    go: (...parts) => { location.hash = ctx.href(...slots(parts)); },
  };
  const root = h("div", { class: "hn" });
  fill(el, root);

  // The bar: the box, with completions, and the pages.
  const input = h("input", { type: "search", placeholder: "Search stories and comments: try  rust by:steveklabnik  or  postgres vs mysql after:2020", autocomplete: "off", spellcheck: "false", "aria-label": "Search Hacker News" });
  const sugg = h("div", { class: "hn-sugg" });
  const fastBtn = h("button", { class: "hn-fast", title: "Fast mode: rank by words alone (BM25), no meaning and no reranking. Applies to every search until switched off.", "aria-pressed": "false" });
  const drawFast = () => { fill(fastBtn, h("span", { class: "dot" }), "Fast"); fastBtn.classList.toggle("on", fastOn); fastBtn.setAttribute("aria-pressed", String(fastOn)); };
  drawFast();
  fastBtn.addEventListener("click", () => {
    fastOn = !fastOn;
    try { localStorage.setItem("hn.fast", fastOn ? "1" : "0"); } catch { /* storage off */ }
    drawFast();
    if (current && (current[0] === "search" || (current[0] || "") === "")) show(current);
  });
  const nav = h("nav", { class: "hn-nav" });
  const live = h("span", { class: "live" });
  const top = h("div", { class: "hn-top" },
    h("div", { class: "hn-bar" },
      h("a", { class: "hn-brand", href: app.href(""), html: `${ICON}<span>HN Search <small>on ChloeDB</small></span>` }),
      h("div", { class: "hn-box" }, h("span", { html: SEARCH, style: { display: "contents" } }), input, h("span", { class: "kbd" }, "/"), sugg),
      fastBtn),
    nav);
  const main = h("main", { class: "hn-main" });
  const sqlBtn = h("button", { onclick: () => sqlPanel.classList.toggle("on"), title: "The SQL behind this page" });
  const sqlPanel = h("div", { class: "hn-sqlp" });
  root.append(top, main, h("div", { class: "hn-sql" }, sqlBtn), sqlPanel,
    h("div", { class: "hn-foot" }, "Hacker News stories and comments from the public archive (open-index/hacker-news, ODC-By) and the official HN API, kept in ChloeDB tables. Not affiliated with Y Combinator. ",
      h("a", { href: app.href("about") }, "How this works")));
  [...top.querySelectorAll(".hn-box > span:first-child")].forEach((s) => { s.firstChild && s.replaceWith(s.firstChild); });

  function drawSql() {
    const ms = app.queries.reduce((a, q) => a + (q.ms || 0), 0);
    fill(sqlBtn, "SQL · ", h("b", {}, String(app.queries.length)), ` ${app.queries.length === 1 ? "query" : "queries"} · ${Math.round(app.took || ms)} ms`);
    fill(sqlPanel, h("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "6px" } },
      h("b", {}, "The SQL behind this page"), h("button", { class: "hn-btn ghost", style: { padding: "3px 9px" }, onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, q.part ? h("i", { class: "pt" }, q.part) : null, q.what, h("span", { title: q.server_ms != null ? `the server's own time ${q.server_ms.toFixed(1)} ms; the rest is the network and the wait for the server` : "" }, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`, q.server_ms != null ? h("small", { class: "sv" }, ` (server ${q.server_ms.toFixed(1)} ms)`) : null)),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", {}, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(q.params)}` : ""))));
  }
  drawSql();

  const PAGES = [["", "Search"], ["front", "Front page"], ["trends", "Trends"], ["link", "Links"], ["digest", "Weekly"], ["people", "People"], ["ask", "What HN thinks"], ["about", "How it works"]];
  function drawNav(page) {
    fill(nav, ...PAGES.map(([p, t]) => h("a", { href: app.href(p), class: (page === p || (p === "" && (page === "search" || page === "item"))) ? "on" : "" }, t)), h("span", { class: "sp" }), live);
  }

  // Typing searches at once; Enter or a completion commits it to history.
  let typed = 0, suggs = [], sel = -1;
  function search(q, push) {
    const path = q.trim() ? ["search", q.trim()] : [""];
    if (push) { closeSugg(); app.go(...path); return null; }
    history.replaceState(null, "", app.href(...path));
    return show(path);
  }
  // The completions close once the search they were typed for is on the
  // page, and a completion that answers after that is dropped.
  function closeSugg() { ++sq; sel = -1; sugg.classList.remove("on"); }
  input.addEventListener("input", () => {
    clearTimeout(typed);
    typed = setTimeout(() => {
      const v = input.value;
      Promise.resolve(search(v, false)).catch(() => {}).finally(() => { if (input.value === v) closeSugg(); });
    }, 140);
    suggest(input.value);
  });
  window.addEventListener("hashchange", closeSugg);
  input.addEventListener("keydown", (e) => {
    const items = [...sugg.querySelectorAll(".it")];
    if (e.key === "ArrowDown" && items.length) { sel = (sel + 1) % items.length; items.forEach((x, i) => x.classList.toggle("sel", i === sel)); e.preventDefault(); }
    else if (e.key === "ArrowUp" && items.length) { sel = (sel - 1 + items.length) % items.length; items.forEach((x, i) => x.classList.toggle("sel", i === sel)); e.preventDefault(); }
    else if (e.key === "Enter") { clearTimeout(typed); const pick = sel >= 0 ? suggs[sel] : null; closeSugg(); if (pick) pick(); else search(input.value, true); }
    else if (e.key === "Escape") { closeSugg(); }
  });
  input.addEventListener("blur", () => setTimeout(closeSugg, 150));
  document.addEventListener("keydown", (e) => {
    if (e.key === "/" && document.activeElement !== input && !/input|textarea/i.test(document.activeElement?.tagName || "")) { e.preventDefault(); input.focus(); input.select(); }
  });
  let sq = 0;
  async function suggest(q) {
    const my = ++sq;
    sel = -1;
    if (q.trim().length < 2) { sugg.classList.remove("on"); return; }
    let j;
    try { j = await ctx.api(`/v1/suggest?q=${enc(q)}`); } catch { return; }
    if (my !== sq || document.activeElement !== input) return;
    suggs = [];
    const rows = [];
    const typed = q.trim().toLowerCase();
    // What is typed in plain weight, the completion in bold.
    const done = (text) => {
      const t = String(text);
      return t.toLowerCase().startsWith(typed) ? [t.slice(0, typed.length), h("b", {}, t.slice(typed.length))] : t;
    };
    // A question left open for its topic ("how many stories mention ") goes in the box to be finished.
    const go = (text) => () => {
      if (text.endsWith(" ")) { input.value = text; input.focus(); suggest(text); return; }
      input.value = text; closeSugg(); search(text, true);
    };
    const head = (t) => rows.push(h("div", { class: "hd" }, t));
    const qs = j.questions || [];
    if (qs.length) head("Ask");
    for (const t of qs) {
      suggs.push(go(t));
      rows.push(h("div", { class: "it", onmousedown: (e) => { e.preventDefault(); go(t)(); } }, h("span", { class: "q" }, "?"), h("span", { class: "t" }, done(t.endsWith(" ") ? `${t}…` : t))));
    }
    const ph = (j.phrases || []).slice(0, 5);
    if (ph.length) head(ph[0].fixed ? "Did you mean" : "Search");
    for (const p of ph) {
      suggs.push(go(p.text));
      rows.push(h("div", { class: "it", onmousedown: go(p.text) }, h("span", { class: "q", style: { background: "var(--surface-2)", color: "var(--muted)" } }, "⌕"), h("span", { class: "t" }, done(p.text)), h("span", { class: "m" }, `${compact(p.n)} ${p.n === 1 ? "story" : "stories"}`)));
    }
    const people = (j.people || []).slice(0, 3), sites = (j.sites || []).slice(0, 2);
    if (people.length || sites.length) head("People and sites");
    for (const p of people) {
      suggs.push(() => { closeSugg(); app.go("user", p.text); });
      rows.push(h("div", { class: "it", onmousedown: () => { closeSugg(); app.go("user", p.text); } }, ava(p.text), h("span", { class: "t" }, p.text), h("span", { class: "m" }, `${compact(p.n)} posts`)));
    }
    for (const s2 of sites) {
      const t = `site:${s2.text}`;
      suggs.push(go(t));
      rows.push(h("div", { class: "it", onmousedown: go(t) }, h("span", { class: "q", style: { background: "var(--surface-2)", color: "var(--muted)" } }, "↗"), h("span", { class: "t" }, s2.text), h("span", { class: "m" }, `${compact(s2.n)} stories`)));
    }
    const st = (j.stories || []).slice(0, 3);
    if (st.length) head("Stories");
    for (const s of st) {
      suggs.push(() => { closeSugg(); app.go("item", s.id); });
      rows.push(h("div", { class: "it", onmousedown: () => { closeSugg(); app.go("item", s.id); } }, h("span", { class: "t" }, s.title), h("span", { class: "m" }, `${fmt(s.points)} points · ${new Date(s.created).getFullYear()}`)));
    }
    fill(sugg, ...rows);
    sugg.classList.toggle("on", rows.length > 0);
  }

  // Pages by path.
  let current = null;
  async function show(path) {
    path = path || ctx.path() || [];
    const [page = "", ...kept] = path;
    const rest = kept.map((x) => (x === "-" ? "" : x));
    current = path;
    drawNav(page);
    const my = ++app.seq;
    const alive = () => my === app.seq;
    if (page !== "search" && page !== "") input.value = page === "item" || page === "user" ? input.value : "";
    if (page === "search" && document.activeElement !== input) input.value = (rest[0] || "");
    try {
      if (page === "" || page === "search") {
        const q = page === "search" ? (rest[0] || "") : input.value;
        if (!q.trim()) return home(main, app, alive);
        return await results(main, app, q, rest.slice(1), alive);
      }
      if (page === "item") return await thread(main, app, Number(rest[0]), alive);
      if (page === "user") return await person(main, app, (rest[0] || ""), alive);
      if (page === "people") return await experts(main, app, (rest[0] || ""), alive);
      if (page === "trends") return await trends(main, app, (rest[0] || ""), alive);
      if (page === "ask") return await ask(main, app, (rest[0] || ""), alive);
      if (page === "front") return await front(main, app, alive);
      if (page === "link") return await linkPage(main, app, (rest[0] || ""), alive);
      if (page === "digest") return await digest(main, app, (rest[0] || ""), alive);
      if (page === "about") return about(main, app);
      fill(main, h("div", { class: "hn-empty" }, "No such page."));
    } catch (e) {
      if (alive()) fill(main, h("div", { class: "hn-err" }, String(e.message || e)));
    }
  }
  app.show = show;
  app.input = input;
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) { closeSugg(); show(p); } });
  show(ctx.path());

  // The feed's state, by the bar.
  const tick = async () => {
    try {
      const s = await ctx.api("/v1/status");
      app.status = s;
      const f = s.feed;
      const n = (s.tables?.hn_stories || 0) + (s.tables?.hn_comments || 0);
      fill(live, f?.running ? h("span", { class: "hn-dot" }) : null, f?.running ? `live · ${compact(n)} items` : `${compact(n)} items`);
      live.title = f?.running ? `The HN API is read every half minute: ${fmt(f.written)} items written, ${fmt(f.refreshed)} stories refreshed${f.behind ? `, ${fmt(f.behind)} behind` : ""}` : "Loaded from the archive";
    } catch { /* the bar stays as it is */ }
  };
  tick();
  const timer = setInterval(tick, 30000);
  return () => clearInterval(timer);
}

// ---- results ---------------------------------------------------------------

// The answer to a question asked in a sentence: read by a grammar, answered
// by SQL (the drawer shows it), no model.
function answerCard(app, a, re, q, j) {
  const sqlNote = h("span", { class: "nm", title: "Read by a grammar, not a model; answered by the SQL in the drawer", onclick: () => document.querySelector(".hn-sql button")?.click() }, "no model · see the SQL");
  const ey = h("div", { class: "ey" }, "Answer", sqlNote);
  const kids = [ey];
  const bars = (rows, peak) => {
    if (!rows?.length) return null;
    const max = Math.max(...rows.map((r) => r.v));
    return [h("div", { class: "yrs" }, rows.map((r) => h("div", { class: r.k === peak ? "pk" : "", style: { height: `${Math.max(3, (r.v / max) * 100)}%` }, title: r.t }))),
      h("div", { class: "yl" }, h("span", {}, rows[0].k), peak ? h("span", {}, `peak ${peak}`) : null, h("span", {}, rows[rows.length - 1].k))];
  };
  if (a.kind === "top") {
    kids.push(h("div", { class: "q" }, a.title));
    const one = (x) => a.item_kind === "comment"
      ? [h("div", { class: "big" }, h("a", { href: app.href("item", x.id) }, `${x.author} on “${x.story_title || "a story"}”`)),
         h("div", { class: "meta" }, h("span", {}, h("b", {}, fmt(x.thread || x.replies || 0)), " replies under it"), h("span", {}, day(x.created))),
         h("div", { class: "txt" }, marked(excerpt(x.body || "", re, 600), re))]
      : [h("div", { class: "big" }, h("a", { href: app.href("item", x.id) }, String(x.title || "").replace(/^(Show|Ask|Launch|Tell) HN:\s*/i, (m) => m))),
         h("div", { class: "meta" }, h("span", {}, h("b", {}, fmt(x.comments)), " comments"), h("span", {}, h("b", {}, fmt(x.points)), " points"), h("a", { href: app.href("user", x.author) }, x.author), h("span", {}, day(x.created)), x.site ? h("span", {}, x.site) : null)];
    if (a.one || !(a.items || []).length) kids.push(...one(a.item));
    else kids.push(h("ol", {}, a.items.map((x) => h("li", {}, h("div", { class: "who" }, h("a", { href: app.href("item", x.id) }, a.item_kind === "comment" ? `${x.author} on “${x.story_title}”` : x.title)), h("span", { class: "n" }, a.by === "points" ? `${fmt(x.points)} points` : a.by === "comments" || a.by === "least_comments" ? `${fmt(a.item_kind === "comment" ? x.thread : x.comments)} ${a.item_kind === "comment" ? "replies" : "comments"}` : day(x.created))))));
  } else if (a.kind === "count") {
    kids.push(h("div", { class: "num" }, fmt(a.n)), h("div", { class: "q" }, a.title.replace(/^[\d,]+\s*/, "")));
    const ys = (a.years || []).filter((y) => y.value >= 2006).map((y) => ({ k: y.value, v: y.n, t: `${y.value}: ${fmt(y.n)}` }));
    kids.push(bars(ys));
  } else if (a.kind === "people") {
    kids.push(h("div", { class: "q" }, a.title));
    const what = a.what === "points" ? "points" : a.what === "comments" ? "comments" : a.what === "stories" ? "stories" : "comments on it";
    kids.push(h("ol", {}, (a.people || []).map((p) => h("li", {}, h("div", { class: "who" }, ava(p.author), h("a", { href: app.href("user", p.author) }, p.author)), h("span", { class: "n" }, `${fmt(p.n)} ${what}`)))));
    if (a.note) kids.push(h("div", { class: "note" }, a.note));
  } else if (a.kind === "sites") {
    kids.push(h("div", { class: "q" }, a.title));
    const topic = (j.understood?.text || "").trim();
    kids.push(h("ol", {}, (a.sites || []).map((s) => h("li", {}, h("div", { class: "who" }, h("a", { href: app.href("search", `${topic} site:${s.site}`.trim()) }, s.site)), h("span", { class: "n" }, `${fmt(s.n)} stories`)))));
  } else if (a.kind === "when") {
    kids.push(h("div", { class: "big" }, a.title));
    if (a.more) kids.push(h("div", { class: "meta" }, a.more));
    const ys = (a.years || []).filter((y) => y.year >= 2006).map((y) => ({ k: y.year, v: y.share, t: `${y.year}: ${fmt(y.n)} stories, ${(100 * y.share).toFixed(2)}%` }));
    kids.push(bars(ys, a.peak));
    kids.push(h("div", { class: "note" }, "Bars are each year's share of all stories posted that year: HN grew, so counts alone would favour recent years."));
  } else if (a.kind === "opinion") {
    // The model's answer, asked for now and written in when it comes; the
    // results below do not wait for it.
    return answerBox(app, q, () => true, { title: a.title, compact: true });
  }
  return h("div", { class: "hn-ans" }, kids);
}

function storyItem(app, s, re, extra) {
  const k = s.kind && s.kind !== "story" ? h("span", { class: `k ${s.kind}` }, s.kind === "show" ? "Show HN" : s.kind === "ask" ? "Ask HN" : s.kind === "launch" ? "Launch HN" : s.kind === "tell" ? "Tell HN" : s.kind) : null;
  const title = String(s.title || "").replace(/^(Show|Ask|Launch|Tell) HN:\s*/i, "");
  const topics = (Array.isArray(s.topics) ? s.topics : []).slice(0, 3);
  return h("li", { class: "hn-s" },
    h("div", { class: "hn-pts" }, h("b", {}, compact(s.points)), h("span", {}, "points")),
    h("div", {},
      h("div", { class: "hn-ttl" }, s.title_hl ? h("a", { href: app.href("item", s.id), html: s.title_hl }) : h("a", { href: app.href("item", s.id) }, marked(title, re)), s.site ? h("a", { class: "site", href: app.href("search", `site:${s.site}`) }, s.site) : null),
      h("div", { class: "hn-meta" }, k,
        h("a", { href: app.href("item", s.id) }, `${fmt(s.comments)} comments`),
        h("a", { href: app.href("user", s.author) }, s.author),
        h("span", { title: day(s.created) }, ago(s.created)),
        topics.map((t) => h("a", { class: "tp", href: app.href("search", `topic:${t.replace(/\s+/g, "-")}`) }, t)),
        s.url ? h("a", { href: s.url, target: "_blank", rel: "noopener noreferrer", title: s.url }, "link ↗") : null,
        s.url ? h("a", { href: app.href("link", s.url), title: "Every discussion of this link, and who was there" }, "across HN") : null),
      s.body_hl ? h("div", { class: "hn-snip", html: s.body_hl }) : s.body ? h("div", { class: "hn-snip" }, marked(excerpt(s.body, re, 240), re)) : null,
      extra));
}

function commentItem(app, c, re) {
  // The server's highlight (the field's stems, synonyms and typos), else marked here.
  const txt = c.body_hl ? h("div", { class: "txt", html: c.body_hl }) : h("div", { class: "txt" }, marked(excerpt(c.body, re, 700), re));
  return h("li", { class: "hn-c" },
    h("div", { class: "on" }, "on ", h("a", { href: app.href("item", c.story) }, c.story_title || `story ${c.story}`)),
    h("div", { class: "hn-meta" },
      h("a", { href: app.href("user", c.author), style: { color: "var(--text-2)", fontWeight: 600 } }, c.author),
      h("span", { title: day(c.created) }, ago(c.created)),
      c.parent_author && c.depth > 1 ? h("span", {}, `↳ replying to ${c.parent_author}`) : h("span", {}, "top-level"),
      c.thread > 0 ? h("span", {}, `${fmt(c.thread)} ${c.thread === 1 ? "reply" : "replies"} under it`) : null,
      h("a", { href: app.href("item", c.id), style: { color: "var(--accent)" } }, "in context →")),
    txt);
}

const skel = (n) => Array.from({ length: n }, () => h("div", { class: "hn-skel" }));

function facetBox(app, title, rows, opts) {
  if (!rows?.length) return null;
  const max = Math.max(...rows.map((r) => r.n));
  return h("div", { class: "hn-facet" }, h("h4", {}, title),
    rows.slice(0, opts.limit || 8).map((r) => h("div", { class: `r ${opts.on?.(r.value) ? "on" : ""}`, onclick: () => opts.pick(r.value) },
      h("span", { class: "bar" }, opts.label ? opts.label(r.value) : String(r.value)),
      h("span", { class: "n" }, compact(r.n)),
    )).map((x, i) => { x.style.background = `linear-gradient(90deg, color-mix(in srgb, var(--accent) 8%, transparent) ${Math.max(2, (rows[i].n / max) * 100)}%, transparent 0)`; return x; }));
}

function years(app, rows, q) {
  if (!rows?.length) return null;
  const ys = rows.filter((r) => r.value >= 2006);
  if (!ys.length) return null;
  const first = ys[0].value, last = new Date().getFullYear();
  const by = new Map(ys.map((r) => [r.value, r.n]));
  const max = Math.max(...ys.map((r) => r.n));
  const bars = [];
  for (let y = first; y <= last; y++) {
    const n = by.get(y) || 0;
    bars.push(h("div", { style: { height: `${Math.max(3, (n / max) * 100)}%`, opacity: n ? 1 : 0.25 }, title: `${y}: ${fmt(n)}`, onclick: () => app.go("search", addOp(q, `year:${y}`, /\b(year|in|after|before|since):\S+/i)) }));
  }
  return h("div", { class: "hn-facet" }, h("h4", {}, "When"), h("div", { class: "hn-years" }, bars), h("div", { class: "hn-yl" }, h("span", {}, first), h("span", {}, last)));
}

function addOp(q, op, replace) {
  let s = q;
  if (replace) s = s.replace(replace, "").replace(/\s+/g, " ").trim();
  return `${s} ${op}`.trim();
}

// A path's empty slots (the search's options: only, sort, mode) travel as
// "-", since a path drops empty parts and the slots after would move up;
// empty slots at the end are left off.
function slots(parts) {
  const p = [...parts];
  while (p.length > 1 && (p[p.length - 1] === "" || p[p.length - 1] == null)) p.pop();
  return p.map((x, i) => (i > 0 && (x === "" || x == null) ? "-" : x));
}

async function results(main, app, q, opts, alive) {
  const [only = "", sort = "", picked = ""] = opts;
  // The bar's fast switch, unless this search picked its own way to match.
  const mode = picked || (app.fast ? "fast" : "");
  const head = h("div", { class: "hn-head" });
  const body = h("div", {}, skel(5));
  const left = h("aside", { class: "hn-left" });
  const right = h("aside", { class: "hn-right" });
  const und = h("div", { class: "hn-und" });
  fill(main, und, h("div", { class: "hn-grid" }, left, h("section", {}, head, body), right));
  const base = `q=${enc(q)}${sort ? `&sort=${sort}` : ""}${mode ? `&mode=${mode}` : ""}`;
  const qs = `${base}${only ? `&only=${only}` : ""}`;
  // The stories first. The comments are asked for at the same time and
  // shown when they come; the facets once the results are on the page.
  const all = !only;
  const cP = all ? app.ctx.api(`/v1/search?${base}&only=comment&per=8&facets=0`).catch(() => null) : null;
  const j = await app.api(`/v1/search?${qs}&facets=0${all ? "&comments=0" : ""}`);
  if (!alive()) return;
  const u = j.understood || {};
  const re = markers(u);
  // What the box understood, each part removable.
  const chips = [];
  for (const n of u.notes || []) chips.push(h("a", { class: "hn-chip f", title: `from “${n.from}”: remove`, onclick: () => app.go("search", q.replace(n.from, "").replace(/\s+/g, " ").trim() || " ") }, n.what, h("span", { class: "x" }, "×")));
  if (u.text) chips.unshift(h("span", { class: "hn-chip", style: { cursor: "default" } }, h("span", { class: "from" }, "words"), u.text));
  for (const p of u.phrases || []) chips.push(h("span", { class: "hn-chip", style: { cursor: "default" } }, h("span", { class: "from" }, "phrase"), `“${p}”`));
  for (const x of u.exclude || []) chips.push(h("span", { class: "hn-chip", style: { cursor: "default" } }, h("span", { class: "from" }, "without"), x));
  fill(und, chips.length ? h("span", {}, "Understood as") : null, chips);

  const set = (o, s, m) => app.go("search", q, o ?? only, s ?? sort, m ?? picked);
  const seg = (items, cur, pick) => h("div", { class: "hn-seg" }, items.map(([v, t]) => h("button", { class: (cur || items[0][0]) === v ? "on" : "", onclick: () => pick(v) }, t)));
  const ts = j.total_stories || 0;
  let tc = j.total_comments || 0;
  const fast = j.mode === "fast";
  fill(head,
    seg([["all", "All"], ["story", `Stories ${ts > 0 ? compact(ts) : ""}`], ["comment", `Comments ${tc > 0 ? compact(tc) : ""}`]], j.only, (v) => set(v === "all" ? "" : v)),
    h("span", { class: "sp" }),
    seg([["best", "Best"], ["points", "Points"], ["date", "Newest"], ["comments", "Discussed"]], j.sort, (v) => set(null, v === "best" ? "" : v)),
  );
  const list = [];
  if (j.did_you_mean) list.push(h("div", { class: "hn-dym" }, "Did you mean ", h("a", { href: app.href("search", j.did_you_mean) }, j.did_you_mean), "?"));
  if (j.answer) list.push(answerCard(app, j.answer, re, q, j));
  const countLine = h("span", { class: "t" });
  const byMeaning = j.mode === "meaning";
  const nStories = (j.stories || []).length;
  const storyCount = () => fast ? [h("b", {}, "fast"), " · BM25 of the title alone, no ranking, no count"]
    : byMeaning ? [h("b", {}, "by meaning"), " · the closest stories first, no count"]
    : ts < 0 || (ts === 0 && nStories) ? [h("b", {}, `${fmt(nStories)}+`), " stories"]
    : [h("b", {}, fmt(ts)), " stories"];
  const drawCount = () => fill(countLine, storyCount(), !fast && tc > 0 ? [" · ", h("b", {}, fmt(tc)), " comments"] : "", ` · found in ${Math.round(j.took_ms)} ms`);
  drawCount();
  list.push(h("div", { class: "hn-head", style: { margin: "8px 0 2px" } }, countLine, h("span", { class: "sp" }),
    h("span", { class: "t", style: { fontSize: "12px" } }, "Match by "), seg([["hybrid", "words + meaning"], ["words", "words"], ["meaning", "meaning"], ["exact", "exact text"], ["fast", "fast"]], j.mode, (v) => set(null, null, v === "hybrid" && !app.fast ? "" : v))));
  // The same link submitted again folds under its best discussion.
  const seen = new Map();
  const stories = [];
  for (const s of j.stories || []) {
    const key = s.url ? s.url.replace(/^https?:\/\/(www\.)?/, "").replace(/[#?].*$/, "").replace(/\/$/, "") : `#${s.id}`;
    if (seen.has(key)) { seen.get(key).again.push(s); continue; }
    const e = { s, again: [] };
    seen.set(key, e);
    stories.push(e);
  }
  if (stories.length) {
    list.push(h("ul", { class: "hn-res" }, stories.map(({ s, again }) => storyItem(app, s, re, again.length ? h("div", { class: "hn-again" }, `Also discussed ${again.length === 1 ? "once more" : `${again.length} more times`}: `, again.map((a, i) => [i ? ", " : "", h("a", { href: app.href("item", a.id) }, `${new Date(a.created).getFullYear()} (${fmt(a.comments)} comments)`)])) : null))));
  }
  // The comments: on their own page, with the results; beside the
  // stories, a placeholder they fill when they come.
  const cBox = h("div", {});
  if (all) {
    fill(cBox, h("div", { class: "hn-sec" }, h("h3", {}, "From the comments"), h("span", { class: "t" }, "searching 40 million comments…")), skel(2));
    list.push(cBox);
    cP.then((k) => {
      if (!alive()) return;
      app.add(k, "comments");
      if (!k || !(k.comments || []).length) { cBox.remove(); if (!stories.length) fill(body, h("div", { class: "hn-empty" }, "Nothing matched. ", j.did_you_mean ? "" : "Try fewer words, or Meaning above.")); return; }
      tc = k.total_comments || 0;
      drawCount();
      fill(cBox, h("div", { class: "hn-sec" }, h("h3", {}, "From the comments"), h("a", { href: app.href("search", q, "comment") }, tc > 0 ? `All ${fmt(tc)} comments →` : "All comments →")),
        h("ul", { class: "hn-res" }, k.comments.map((c) => commentItem(app, c, re))));
    });
  } else if ((j.comments || []).length) {
    list.push(h("ul", { class: "hn-res" }, j.comments.map((c) => commentItem(app, c, re))));
  }
  if (!stories.length && !(j.comments || []).length && !all) list.push(h("div", { class: "hn-empty" }, "Nothing matched. ", j.did_you_mean ? "" : "Try fewer words, or Meaning above."));
  else {
    let page = 0;
    const more = h("button", { class: "hn-more", onclick: async () => {
      page++;
      more.textContent = "Loading…";
      const k = app.add(await app.ctx.api(`/v1/search?${qs}&page=${page}&facets=0`), `page ${page + 1}`);
      const add2 = [...(k.stories || []).map((s) => storyItem(app, s, re)), ...(k.comments || []).map((c) => commentItem(app, c, re))];
      more.before(h("ul", { class: "hn-res" }, add2));
      more.textContent = "More results";
      if (!add2.length) more.remove();
    } }, "More results");
    list.push(more);
  }
  fill(body, ...list);

  // Facets: asked for once the results are shown; each a click that
  // narrows the query, by its operator.
  fill(left, h("div", { class: "hn-facet" }, h("h4", {}, "When"), h("div", { class: "hn-skel", style: { height: "54px" } })), h("div", { class: "hn-facet" }, h("h4", {}, "Type"), h("div", { class: "hn-skel", style: { height: "90px" } })));
  const words = (u.text || "").trim();
  const spark = h("div", {}, h("div", { class: "hn-skel", style: { height: "60px" } }));
  fill(right, 
    words ? h("a", { class: "hn-ask", href: app.href("ask", `What does HN think about ${words}?`) }, h("span", { html: SPARK, style: { display: "contents" } }), h("div", {}, h("b", {}, `What does HN think about ${words}?`), h("span", {}, "A short answer from the comments, every claim cited"))) : null,
    words ? h("div", { class: "hn-card" }, h("h4", {}, "Talked about, year by year", h("a", { href: app.href("trends", words) }, "Trends →")), spark) : null,
    words ? peopleCard(app, words) : null,
  );
  if (fast) { fill(left); fill(spark, h("div", { style: { fontSize: "12.5px", color: "var(--muted)" } }, "Fast mode skips the facets.")); return; }
  const fj = await app.ctx.api(`/v1/facets?${qs}`).catch(() => null);
  if (!alive()) return;
  app.add(fj, "facets");
  const f = fj?.facets || {};
  fill(spark, sparkYears(f.year));
  const KIND = { story: "Story", show: "Show HN", ask: "Ask HN", launch: "Launch HN", tell: "Tell HN", job: "Job", poll: "Poll" };
  fill(left, 
    years(app, f.year, q),
    facetBox(app, "Type", f.kind, { label: (v) => KIND[v] || v, on: (v) => (u.kinds || []).includes(v), pick: (v) => app.go("search", addOp(q, `type:${v}`, /\btype:\S+/i)) }),
    facetBox(app, "Points", (f.points || []).sort((a, b) => parseInt(b.value) - parseInt(a.value) || (a.value.startsWith("<") ? 1 : -1)), { pick: (v) => app.go("search", addOp(q, v.startsWith("<") ? "points<20" : `points>${parseInt(v) - 1}`, /\bpoints[<>:]\S+/i)) }),
    facetBox(app, "Topics", (f.topic || []).filter((r) => r.value), { on: (v) => u.topic === v, pick: (v) => app.go("search", addOp(q, `topic:${String(v).replace(/\s+/g, "-")}`, /\btopic:\S+/i)) }),
    facetBox(app, "Sites", f.site, { on: (v) => u.site === v, pick: (v) => app.go("search", addOp(q, `site:${v}`, /\bsite:\S+/i)) }),
    facetBox(app, "Posted by", f.author, { on: (v) => u.author === v, pick: (v) => app.go("search", addOp(q, `by:${v}`, /\bby:\S+/i)) }),
  );
  if (!left.children.length) left.append(h("div", { class: "hn-facet" }, h("h4", {}, "Filters"), h("div", { style: { fontSize: "12.5px", color: "var(--muted)" } }, "Type by:name, site:example.com, after:2020, points>100, type:show, or say it: “Show HN posts about databases since 2021”.")));

}

function sparkYears(rows) {
  const ys = (rows || []).filter((r) => r.value >= 2006);
  if (ys.length < 2) return h("div", { style: { fontSize: "12.5px", color: "var(--muted)" } }, "Not enough years yet.");
  const W = 236, H = 60, max = Math.max(...ys.map((r) => r.n));
  const x0 = ys[0].value, x1 = ys[ys.length - 1].value;
  const pts = ys.map((r) => [((r.value - x0) / Math.max(1, x1 - x0)) * (W - 4) + 2, H - 4 - (r.n / max) * (H - 12)]);
  const peak = ys.reduce((a, b) => (b.n > a.n ? b : a));
  return h("div", {}, h("svg", { viewBox: `0 0 ${W} ${H}`, style: { width: "100%", height: "60px" } },
    h("path", { d: `M${pts.map((p) => p.join(",")).join("L")}L${W - 2},${H}L2,${H}Z`, fill: "color-mix(in srgb, var(--accent) 14%, transparent)" }),
    h("polyline", { points: pts.map((p) => p.join(",")).join(" "), fill: "none", stroke: "var(--accent)", "stroke-width": 2, "stroke-linejoin": "round" })),
    h("div", { class: "hn-yl" }, h("span", {}, x0), h("span", {}, `peak ${peak.value}`), h("span", {}, x1)));
}

function peopleCard(app, words) {
  const card = h("div", { class: "hn-card" }, h("h4", {}, "Who knows about it", h("a", { href: app.href("people", words) }, "People →")), h("div", { class: "hn-skel", style: { height: "40px" } }));
  app.ctx.api(`/v1/experts?q=${enc(words)}`).then((j) => {
    const ex = (j.experts || []).slice(0, 8);
    card.lastChild.replaceWith(ex.length ? h("div", { class: "hn-ppl" }, ex.map((e) => h("a", { href: app.href("user", e.id), title: `${e.comments_on_topic} comments on it, in ${e.threads} threads` }, ava(e.id), e.id))) : h("div", { style: { fontSize: "12.5px", color: "var(--muted)" } }, "No one stands out yet."));
  }).catch(() => card.remove());
  return card;
}

// ---- home ------------------------------------------------------------------

const TRY = [
  ["rust by:steveklabnik", "a person's words"],
  ["postgres vs mysql after:2020", "words, and a date"],
  ["Show HN databases with over 300 points", "said as a sentence"],
  ["kubernets", "a typo"],
  ["most discussed stories about remote work", "sorted by what you say"],
  ["\"memory safety\" site:github.com", "a phrase, on a site"],
];

function home(main, app, alive) {
  const s = app.status;
  const stats = h("div", { class: "hn-stats" });
  const drawStats = (st) => fill(stats, 
    h("div", {}, h("b", {}, compact(st?.tables?.hn_stories || 0)), h("span", {}, "stories")),
    h("div", {}, h("b", {}, compact(st?.tables?.hn_comments || 0)), h("span", {}, "comments")),
    h("div", {}, h("b", {}, compact(st?.tables?.hn_users || 0)), h("span", {}, "people")),
    h("div", {}, h("b", {}, compact(st?.tables?.hn_replies || 0)), h("span", {}, "reply edges")),
    h("div", {}, h("b", {}, st?.stories?.first ? new Date(st.stories.first).getFullYear() : "2006"), h("span", {}, "since")));
  drawStats(s);
  const frontBody = h("div", {}, skel(3));
  const frontCol = h("div", { class: "hn-card" }, h("h4", {}, "On the front page now", h("a", { href: app.href("front") }, "All →")), frontBody);
  const askCol = h("div", { class: "hn-card" }, h("h4", {}, "Ask the archive"),
    h("ul", { class: "hn-mini" }, ["What does HN think about SQLite in production?", "Is Rust worth it for a small team?", "What do people regret about microservices?", "How do founders find their first customers?"].map((t) =>
      h("li", {}, h("a", { href: app.href("ask", t) }, t), h("span", { class: "m" }, "an answer from the comments, cited")))));
  const trendBody = h("div", {}, skel(3));
  const trendCol = h("div", { class: "hn-card" }, h("h4", {}, "Trending now", h("a", { href: app.href("trends") }, "Trends →")), trendBody);
  app.ctx.api("/v1/trending?hours=48").then((j) => {
    if (!alive()) return;
    if (!j?.terms?.length) { trendBody.replaceWith(h("ul", { class: "hn-mini" }, [["rust, go, zig", "languages"], ["kubernetes, docker, serverless", "infrastructure"], ["gpt, llm, copilot", "AI"]].map(([t, w]) => h("li", {}, h("a", { href: app.href("trends", t) }, t), h("span", { class: "m" }, w))))); return; }
    trendBody.replaceWith(h("ul", { class: "hn-mini" }, j.terms.slice(0, 6).map((t) => h("li", {}, h("a", { href: app.href("search", t.term, "", "date") }, t.term), h("span", { class: "m" }, `×${t.ratio >= 100 ? Math.round(t.ratio) : t.ratio} its usual share · ${fmt(t.n)} stories in 48 h`)))));
  }).catch(() => trendBody.remove());
  fill(main, 
    h("div", { class: "hn-hero" },
      h("h1", {}, "Search every ", h("em", {}, "Hacker News"), " discussion"),
      h("p", {}, "Every story and comment since 2006, by words and by meaning, in their threads. Ask what HN thinks, see who knows a topic, and watch topics rise and fall."),
      h("div", { class: "hn-try" }, TRY.map(([q, w]) => h("a", { href: app.href("search", q) }, h("small", {}, w), h("code", {}, q))))),
    stats,
    h("div", { class: "hn-cols" }, frontCol, askCol, trendCol));
  app.input.focus();
  app.ctx.api("/v1/status").then((st) => { if (alive()) drawStats(st); }).catch(() => {});
  app.api("/v1/front").then((j) => {
    if (!alive()) return;
    frontBody.replaceWith(h("ul", { class: "hn-mini" }, (j.stories || []).slice(0, 6).map((s) =>
      h("li", {}, h("a", { href: app.href("item", s.id) }, s.title), h("span", { class: "m" }, `${fmt(s.points)} points · ${fmt(s.comments)} comments · ${ago(s.created)}`)))));
  }).catch(() => frontBody.remove());
}

// ---- a discussion ----------------------------------------------------------

async function thread(main, app, id, alive) {
  fill(main, h("div", { class: "hn-grid two" }, h("section", {}, skel(6)), h("aside", {})));
  const j = await app.api(`/v1/item/${id}`);
  if (!alive()) return;
  const s = j.story;
  const re = markers({ text: app.input.value.replace(/\b\w+:\S+/g, "") });
  const tree = h("div", { class: "hn-tree" });
  const kids = new Map();
  for (const c of j.comments || []) {
    const p = c.parent === j.story_id ? 0 : c.parent;
    if (!kids.has(p)) kids.set(p, []);
    kids.get(p).push(c);
  }
  for (const list of kids.values()) list.sort((a, b) => (a.pos ?? 0) - (b.pos ?? 0) || a.id - b.id);
  const path = new Set(j.path || []);
  let focusEl = null;
  const node = (c) => {
    const k = kids.get(c.id) || [];
    const tg = h("span", { class: "tg", title: "Fold" }, "[–]");
    const el = h("div", { class: `hn-cm${c.id === j.focus ? " focus" : ""}${path.has(c.id) ? " path" : ""}`, id: `c${c.id}` },
      h("div", { class: "hd" }, ava(c.author), h("a", { class: "u", href: app.href("user", c.author) }, c.author), h("a", { href: app.href("item", c.id), title: day(c.created) }, ago(c.created)), k.length ? tg : null, c.thread > 3 ? h("span", {}, `${fmt(c.thread)} below`) : null),
      h("div", { class: "bd" }, marked(c.body, re)),
      k.length ? h("div", { class: "kids" }, k.map(node)) : null);
    tg.onclick = () => { el.classList.toggle("shut"); tg.textContent = el.classList.contains("shut") ? `[+${c.thread || k.length}]` : "[–]"; };
    if (c.id === j.focus) focusEl = el;
    return el;
  };
  add(tree, (kids.get(0) || []).map(node));
  const k = s?.kind;
  const side = h("aside", { class: "hn-right", style: { display: "block" } }, h("div", { class: "hn-skel", style: { height: "120px" } }));
  fill(main, h("div", { class: "hn-grid two" },
    h("section", {},
      s ? h("div", { class: "hn-story" },
        h("h1", {}, s.url ? h("a", { href: s.url, target: "_blank", rel: "noopener noreferrer" }, s.title) : s.title, s.site ? h("a", { class: "site", href: app.href("search", `site:${s.site}`), style: { fontSize: "14px", color: "var(--muted)", fontWeight: 400, marginLeft: "8px" } }, s.site) : null),
        h("div", { class: "hn-meta" }, k && k !== "story" ? h("span", { class: `k ${k}` }, k) : null, h("span", {}, `${fmt(s.points)} points`), h("a", { href: app.href("user", s.author) }, s.author), h("span", { title: day(s.created) }, `${day(s.created)} · ${ago(s.created)}`), h("span", {}, `${fmt(s.comments)} comments`),
          h("a", { href: `https://news.ycombinator.com/item?id=${s.id}`, target: "_blank", rel: "noopener noreferrer" }, "on HN ↗"),
          s.url ? h("a", { href: app.href("link", s.url) }, "this link across HN →") : null),
        s.body ? h("div", { class: "body" }, marked(s.body, re)) : null)
        : h("div", { class: "hn-err" }, "This story is older than the months loaded; its comments are below."),
      (j.comments || []).length ? tree : h("div", { class: "hn-empty" }, s?.comments ? "This discussion's comments are not loaded yet." : "No comments.")),
    side));
  if (focusEl) setTimeout(() => focusEl.scrollIntoView({ block: "center", behavior: "smooth" }), 60);
  const a = await app.ctx.api(`/v1/item/${j.story_id}/around`).catch(() => ({}));
  if (!alive()) return;
  const mini = (title, rows, more) => rows?.length ? h("div", { class: "hn-card" }, h("h4", {}, title, more || null), h("ul", { class: "hn-mini" }, rows.slice(0, 6).map((r) =>
    h("li", {}, h("a", { href: app.href("item", r.id) }, r.title), h("span", { class: "m" }, `${fmt(r.points)} points · ${fmt(r.comments)} comments · ${new Date(r.created).getFullYear()}`))))) : null;
  fill(side, 
    a.site?.site ? h("div", { class: "hn-card" }, h("h4", {}, a.site.site, h("a", { href: app.href("search", `site:${a.site.site}`) }, "All →")), h("div", { class: "hn-kpis", style: { marginTop: 0 } }, h("span", {}, h("b", {}, fmt(a.site.stories)), "stories"), h("span", {}, h("b", {}, compact(a.site.points)), "points"), a.site.first ? h("span", {}, "since ", h("b", {}, new Date(a.site.first).getFullYear())) : null)) : null,
    mini(`The same link, discussed again`, a.same_link, s?.url ? h("a", { href: app.href("link", s.url) }, "Map →") : null),
    mini("Discussions this one links to", a.links_to),
    mini("Discussions that link here", a.linked_from),
    mini("Nearest by meaning", a.similar));
}

// ---- a link across HN ------------------------------------------------------

// A small force layout: springs on the edges, every node pushing every
// other away, a pull to the middle; the link is pinned there.
function layout(nodes, edges, W, H) {
  const by = new Map(nodes.map((n) => [n.id, n]));
  // Who and when on the left (submissions, people), where else on the
  // right (discussions citing it, discussions linked to it).
  const ring = { link: 0, story: 0.2, user: 0.4, cites: 0.4, linked: 0.46 };
  const arc = { story: [110, 250], user: [100, 260], cites: [-75, 75], linked: [-50, 50] };
  const side = { story: 0.36, user: 0.26, cites: 0.74, linked: 0.8 };
  const count = {}, seen = {};
  for (const n of nodes) count[n.kind] = (count[n.kind] || 0) + 1;
  nodes.forEach((n) => {
    const [a0, a1] = arc[n.kind] || [0, 360];
    const i = (seen[n.kind] = (seen[n.kind] || 0) + 1);
    const a = ((a0 + ((a1 - a0) * i) / ((count[n.kind] || 1) + 1)) * Math.PI) / 180;
    const r = (ring[n.kind] ?? 0.4) * Math.min(W, H);
    n.x = W / 2 + Math.cos(a) * r * 1.3; n.y = H / 2 + Math.sin(a) * r; n.vx = 0; n.vy = 0;
  });
  const E = edges.map((e) => [by.get(e.a), by.get(e.b), e]).filter(([a, b]) => a && b);
  const len = { submitted: 70, "submitted it": 46, commented: 60, cited: 120, "cited it here": 48, "links to": 70 };
  for (let it = 0; it < 360; it++) {
    const k = 1 - it / 360;
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const a = nodes[i], b = nodes[j];
        let dx = a.x - b.x, dy = a.y - b.y, d2 = dx * dx + dy * dy + 0.01;
        if (d2 > 90000) continue;
        const f = 1400 / d2;
        const d = Math.sqrt(d2);
        dx /= d; dy /= d;
        a.vx += dx * f; a.vy += dy * f; b.vx -= dx * f; b.vy -= dy * f;
      }
    }
    for (const [a, b, e] of E) {
      const dx = b.x - a.x, dy = b.y - a.y, d = Math.sqrt(dx * dx + dy * dy) + 0.01;
      const f = (d - (len[e.kind] || 70)) * 0.03;
      a.vx += (dx / d) * f; a.vy += (dy / d) * f; b.vx -= (dx / d) * f; b.vy -= (dy / d) * f;
    }
    for (const n of nodes) {
      n.vx += ((side[n.kind] ?? 0.5) * W - n.x) * 0.006; n.vy += (H / 2 - n.y) * 0.006;
      if (n.kind === "link") { n.x = W / 2; n.y = H / 2; n.vx = n.vy = 0; continue; }
      const cap = 18 * k + 1;
      n.x += Math.max(-cap, Math.min(cap, n.vx)); n.y += Math.max(-cap, Math.min(cap, n.vy));
      n.vx *= 0.6; n.vy *= 0.6;
      n.x = Math.max(24, Math.min(W - 24, n.x)); n.y = Math.max(20, Math.min(H - 20, n.y));
    }
  }
  return E;
}

function linkGraph(app, g) {
  const nodes = (g?.nodes || []).map((n) => ({ ...n }));
  if (nodes.length < 2) return null;
  const W = 1000, H = Math.min(560, 300 + nodes.length * 4);
  const E = layout(nodes, g.edges || [], W, H);
  const tip = h("div", { class: "tip" });
  const wrap = h("div", { class: "hn-lg" });
  const radius = (n) => n.kind === "link" ? 15 : 5 + Math.max(0, Math.min(1, n.w || 0)) * 9;
  const edgeEls = E.map(([a, b, e]) => ({ a, b, el: h("line", { class: `e ${e.kind === "cited" ? "cited" : e.kind === "links to" ? "links" : ""}`, x1: a.x.toFixed(1), y1: a.y.toFixed(1), x2: b.x.toFixed(1), y2: b.y.toFixed(1) }) }));
  const near = new Map(nodes.map((n) => [n.id, new Set([n.id])]));
  for (const { a, b } of edgeEls) { near.get(a.id).add(b.id); near.get(b.id).add(a.id); }
  const said = (n) => {
    if (n.kind === "link") return [n.label, "the link"];
    if (n.kind === "story") return [n.title, `submitted ${n.label}`];
    if (n.kind === "cites") return [n.title, "a discussion where it was cited"];
    if (n.kind === "linked") return [n.title, "a discussion linked to its discussions"];
    const r = n.roles || {};
    return [n.user, [r.submitted ? `submitted it ${r.submitted === 1 ? "once" : `${r.submitted} times`}` : "", r.commented ? `${fmt(r.commented)} comments in its discussions` : "", r.cited ? `cited it ${r.cited === 1 ? "once" : `${r.cited} times`} elsewhere` : ""].filter(Boolean).join(" · ")];
  };
  const go = (n) => n.kind === "user" ? app.go("user", n.user) : n.item ? app.go("item", n.item) : null;
  const nodeEls = nodes.map((n) => {
    const r = radius(n);
    const show = n.kind === "link" || n.kind === "cites" || (n.kind === "story" && (n.w || 0) > 0.35) || (n.kind === "user" && ((n.roles?.submitted || 0) + (n.roles?.cited || 0)) > 0);
    const lbl = n.kind === "link" ? (n.label.length > 42 ? n.label.slice(0, 41) + "…" : n.label) : n.kind === "story" ? n.label : n.kind === "user" ? n.user : n.label;
    const el = h("g", { class: `n ${n.kind}`, transform: `translate(${n.x.toFixed(1)},${n.y.toFixed(1)})` },
      h("circle", { r: r.toFixed(1) }),
      show ? h("text", { x: (r + 4).toFixed(1), y: "4" }, lbl) : null);
    el.addEventListener("mouseenter", (ev) => {
      const set = near.get(n.id);
      wrap.classList.add("focus");
      nodeEls.forEach((x) => x.el.classList.toggle("hot", set.has(x.n.id)));
      edgeEls.forEach((x) => x.el.classList.toggle("hot", x.a.id === n.id || x.b.id === n.id));
      const [t, m] = said(n);
      fill(tip, h("b", {}, t), h("span", { class: "m" }, m));
      const box = wrap.getBoundingClientRect();
      const sx = box.width / W;
      tip.style.display = "block";
      tip.style.left = `${Math.min(box.width - 300, Math.max(6, n.x * sx + 12))}px`;
      tip.style.top = `${Math.max(6, n.y * sx - 10)}px`;
    });
    el.addEventListener("mouseleave", () => { wrap.classList.remove("focus"); tip.style.display = "none"; });
    el.addEventListener("click", () => go(n));
    return { n, el };
  });
  const svg = h("svg", { viewBox: `0 0 ${W} ${H}`, role: "img", "aria-label": "The link, its submissions, the discussions citing it and the people around it" },
    h("g", {}, edgeEls.map((x) => x.el)), h("g", {}, nodeEls.map((x) => x.el)));
  const dot = (c, t) => h("span", {}, h("i", { style: { background: c } }), t);
  return add(wrap, [svg, tip, h("div", { class: "legend" },
    dot("var(--accent)", "the link"), dot("#f08a24", "a submission (size: points)"), dot("#2fa37a", "a person"), dot("#5b8def", "cited in another discussion"), dot("#9aa3ad", "a discussion linked to it"),
    h("span", { style: { marginLeft: "auto" } }, "hover to see, click to open"))]);
}

function roles(p) {
  return h("span", { class: "hn-roles" },
    p.submitted ? h("span", { class: "s" }, p.submitted === 1 ? "submitted" : `submitted ×${p.submitted}`) : null,
    p.cited ? h("span", { class: "c" }, p.cited === 1 ? "cited it" : `cited ×${p.cited}`) : null,
    p.commented ? h("span", { class: "t" }, `${fmt(p.commented)} comments`) : null);
}

async function linkPage(main, app, u, alive) {
  const box = h("input", { type: "url", placeholder: "Paste a link (or a site, like paulgraham.com) to see it across HN", value: u, "aria-label": "A link" });
  const go = () => { const v = box.value.trim(); if (v) app.go("link", v); };
  box.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
  const head = h("div", { class: "hn-link-box" }, box, h("button", { onclick: go }, "Explore"));
  fill(main, head, h("div", { class: "hn-grid two" }, h("section", {}, skel(5)), h("aside", {})));
  const j = await app.api(`/v1/link?u=${encodeURIComponent(u)}`);
  if (!alive()) return;
  if (j.error) return fill(main, head, h("div", { class: "hn-err" }, j.error));
  const mini = (title, rows, li) => rows?.length ? h("div", { class: "hn-card" }, h("h4", {}, title), h("ul", { class: "hn-mini" }, rows.map(li))) : null;
  if (j.kind === "start") {
    return fill(main, head, h("div", { class: "hn-card" }, h("h4", {}, "The links HN discussed the most"),
      h("ul", { class: "hn-mini" }, (j.links || []).map((r) => h("li", {}, h("a", { href: app.href("link", r.url || r.curl) }, r.title), h("span", { class: "m" }, `${r.curl} · ${fmt(r.comments)} comments · ${fmt(r.points)} points · ${new Date(r.created).getFullYear()}`))))));
  }
  if (j.kind === "site") {
    return fill(main, head,
      h("div", { class: "hn-link-h" }, h("h1", {}, j.site), h("a", { class: "hn-xlink", href: app.href("search", `site:${j.site}`) }, "search the site →")),
      h("div", { class: "hn-kpis" }, h("span", {}, h("b", {}, fmt(j.totals?.stories)), "stories")),
      h("div", { class: "hn-grid two" },
        h("section", {}, h("h3", { class: "hn-h3" }, "Its most discussed links"), h("ul", { class: "hn-res" }, (j.top || []).map((s) => storyItem(app, s, null)))),
        h("aside", { class: "hn-right", style: { display: "block" } },
          mini("Submitted again and again", j.again, (r) => h("li", {}, h("a", { href: app.href("link", r.curl) }, r.curl), h("span", { class: "m" }, `${r.n} submissions`))),
          mini("Who submits it", j.people, (r) => h("li", {}, h("a", { href: app.href("user", r.author) }, r.author), h("span", { class: "m" }, `${fmt(r.n)} stories`))))));
  }
  const t = j.totals || {};
  const first = j.first ? new Date(j.first).getFullYear() : null;
  const cites = (j.cited || []).map((c) => h("div", { class: "hn-cite" },
    h("div", { class: "on" }, h("a", { href: app.href("user", c.author) }, c.author), " on ", h("a", { href: app.href("item", c.story) }, c.story_title || `story ${c.story}`), " · ", h("span", { title: day(c.created) }, ago(c.created)), " · ", h("a", { href: app.href("item", c.id) }, "in context →")),
    h("div", { class: "txt" }, c.text)));
  fill(main, head,
    h("div", { class: "hn-link-h" }, h("h1", {}, h("a", { href: j.url, target: "_blank", rel: "noopener noreferrer" }, j.curl, " ↗")), j.site ? h("a", { class: "hn-xlink", href: app.href("link", j.site) }, `all of ${j.site} →`) : null),
    h("div", { class: "hn-kpis" },
      h("span", {}, h("b", {}, fmt(t.submissions)), t.submissions === 1 ? "submission" : "submissions"),
      h("span", {}, h("b", {}, compact(t.points)), "points"),
      h("span", {}, h("b", {}, compact(t.comments)), "comments"),
      h("span", {}, h("b", {}, fmt(t.cited)), "comments mention it"),
      first ? h("span", {}, "first seen ", h("b", {}, first)) : null),
    t.submissions || t.cited ? linkGraph(app, j.graph) : h("div", { class: "hn-empty" }, "HN has no submission of this link, and no comment that cites it."),
    h("div", { class: "hn-grid two" },
      h("section", {},
        (j.submissions || []).length ? [h("h3", { class: "hn-h3" }, "Every time it was submitted"), h("ul", { class: "hn-res" }, j.submissions.slice().reverse().map((s) => storyItem(app, s, null)))] : null,
        cites.length ? h("div", { class: "hn-card" }, h("h4", {}, `Cited in other discussions${t.cited > cites.length ? ` · the newest ${cites.length} of ${fmt(t.cited)}` : ""}`), cites) : null),
      h("aside", { class: "hn-right", style: { display: "block" } },
        mini("People around it", j.people, (p) => h("li", {}, h("a", { href: app.href("user", p.author) }, p.author), roles(p))),
        mini("Links discussed where it was cited", j.related, (r) => h("li", {}, h("a", { href: app.href("link", r.curl) }, r.title || r.curl), h("span", { class: "m" }, `${r.curl} · cited there ${r.cites === 1 ? "once" : `${r.cites} times`}`))),
        mini("Discussions linked to its discussions", j.linked, (r) => h("li", {}, h("a", { href: app.href("item", r.id) }, r.title), h("span", { class: "m" }, `${fmt(r.points)} points · ${fmt(r.comments)} comments · ${new Date(r.created).getFullYear()}`))))));
}

// ---- the weekly digest -----------------------------------------------------

async function digest(main, app, week, alive) {
  fill(main, h("div", { class: "hn-dg" }, skel(6)));
  const j = await app.api(`/v1/digest${week ? `?week=${encodeURIComponent(week)}` : ""}`);
  if (!alive()) return;
  const t = j.totals || {};
  const intro = h("div", { class: "hn-dg-intro" }, j.intro?.intro || h("span", { class: "wait" }, "The opening is written once the entries are."));
  const boxes = new Map();
  const body = (e, w) => {
    const s = e.story;
    const box = boxes.get(s.id);
    const parts = w ? [
      w.brief ? [h("h5", {}, "In brief"), h("p", {}, w.brief)] : null,
      w.why ? [h("h5", {}, "Why it mattered"), h("p", {}, w.why)] : null,
      w.take ? [h("h5", {}, "What HN made of it"), h("p", {}, w.take)] : null,
      Array.isArray(w.terms) && w.terms.length ? h("div", { class: "terms" }, w.terms.slice(0, 3).map((x) => h("span", {}, x))) : null,
    ] : h("p", { class: "wait" }, "Reading the article and the discussion…");
    fill(box, parts);
  };
  const entries = (j.entries || []).map((e, i) => {
    const s = e.story;
    const box = h("div", {});
    boxes.set(s.id, box);
    const img = e.image ? h("img", { class: "img", src: e.image, alt: "", loading: "lazy", referrerpolicy: "no-referrer", onerror: (ev) => ev.target.remove() }) : null;
    const el = h("article", { class: "hn-dg-e", id: `e${s.id}` },
      img,
      h("div", { class: "n" }, String(i + 1).padStart(2, "0")),
      h("h2", {}, s.url ? h("a", { href: s.url, target: "_blank", rel: "noopener noreferrer" }, s.title, " ↗") : h("a", { href: app.href("item", s.id) }, s.title)),
      h("div", { class: "hn-meta" }, s.site ? h("a", { href: app.href("link", s.site) }, s.site) : null, h("span", {}, `${fmt(s.points)} points`), h("a", { href: app.href("item", s.id) }, `${fmt(s.comments)} comments`), h("a", { href: app.href("user", s.author) }, s.author), s.url ? h("a", { href: app.href("link", s.url) }, "across HN") : null),
      box,
      (e.quotes || []).length ? h("div", { class: "qs" }, h("h5", {}, "In the thread"), e.quotes.map((q) => h("div", { class: "q" }, h("b", {}, h("a", { href: app.href("user", q.author), style: { color: "inherit" } }, q.author)), q.replies ? ` (${fmt(q.replies)} replies) ` : " ", q.text, " ", h("a", { href: app.href("item", q.id) }, "→")))) : null);
    body(e, e.written);
    return el;
  });
  fill(main, h("div", { class: "hn-dg" },
    h("div", { class: "hn-dg-top" }, h("h1", {}, "HN Weekly"), h("span", { class: "wk" }, `${j.label} · ${j.issue}${j.complete ? "" : " · so far"}`),
      h("span", { class: "nv" }, h("a", { href: app.href("digest", j.prev) }, "← earlier"), j.next ? h("a", { href: app.href("digest", j.next) }, "later →") : null)),
    h("div", { class: "hn-kpis" }, h("span", {}, h("b", {}, fmt(t.stories)), "stories"), h("span", {}, h("b", {}, compact(t.points)), "points"), h("span", {}, h("b", {}, compact(t.comments)), "comments"),
      (j.sites || []).slice(0, 4).map((x) => h("span", {}, h("a", { href: app.href("link", x.site) }, x.site)))),
    intro,
    h("ol", { class: "hn-dg-toc" }, (j.entries || []).map((e) => h("li", {}, h("a", { href: `#e${e.story.id}`, onclick: (ev) => { ev.preventDefault(); document.getElementById(`e${e.story.id}`)?.scrollIntoView({ behavior: "smooth" }); } }, e.story.title)))),
    entries));
  // Entries not written yet are written now, one at a time, then the opening.
  const todo = (j.entries || []).filter((e) => !e.written);
  for (const e of todo) {
    if (!alive()) return;
    try {
      const r = await app.api(`/v1/digest/write?week=${j.week}&id=${e.story.id}`);
      if (!alive()) return;
      body(e, r.written);
      if (r.image && !e.image) boxes.get(e.story.id)?.parentElement?.prepend(h("img", { class: "img", src: r.image, alt: "", loading: "lazy", referrerpolicy: "no-referrer", onerror: (ev) => ev.target.remove() }));
    } catch (err) {
      const again = h("a", { href: "#", onclick: async (ev) => {
        ev.preventDefault();
        fill(boxes.get(e.story.id), h("p", { class: "wait" }, "Reading the article and the discussion…"));
        try { body(e, (await app.api(`/v1/digest/write?week=${j.week}&id=${e.story.id}`)).written); }
        catch (x) { fill(boxes.get(e.story.id), h("p", { class: "wait" }, String(x.message || x))); }
      } }, "try again");
      fill(boxes.get(e.story.id), h("p", { class: "wait" }, String(err.message || err), " · ", again));
    }
  }
  if (!j.intro && (j.entries || []).length) {
    try {
      const r = await app.api(`/v1/digest/write?week=${j.week}&id=intro`);
      if (alive() && r.written?.intro) fill(intro, r.written.intro);
    } catch (err) { /* the opening waits for the next visit */ }
  }
}

// ---- a person --------------------------------------------------------------

async function person(main, app, who, alive) {
  fill(main, skel(5));
  const j = await app.api(`/v1/user/${enc(who)}`);
  if (!alive()) return;
  const u = j.user || {};
  const yrs = (j.years || []).map((r) => ({ value: r.year, n: r.n }));
  fill(main, 
    h("div", { class: "hn-pf" }, ava(who, 56), h("div", {}, h("h1", {}, who),
      h("div", { class: "hn-kpis" }, h("span", {}, h("b", {}, fmt(u.comments)), "comments"), h("span", {}, h("b", {}, fmt(u.stories)), "stories"), h("span", {}, h("b", {}, compact(u.points)), "points on stories"), u.first ? h("span", {}, "since ", h("b", {}, new Date(u.first).getFullYear())) : null, h("span", {}, h("b", {}, fmt(u.talked_to)), "people talked to")))),
    h("div", { class: "hn-grid two" },
      h("section", {},
        h("div", { class: "hn-card" }, h("h4", {}, "Their orbit, from the reply graph", h("span", { style: { textTransform: "none", letterSpacing: 0, fontWeight: 400 } }, "personalized PageRank from them")), orbit(app, who, j)),
        j.top?.length ? [h("div", { class: "hn-sec" }, h("h3", {}, "Their best stories"), h("a", { href: app.href("search", `by:${who}`) }, "All →")), h("ul", { class: "hn-res" }, j.top.slice(0, 6).map((s) => storyItem(app, s, null)))] : null,
        j.recent?.length ? [h("div", { class: "hn-sec" }, h("h3", {}, "Latest comments"), h("a", { href: app.href("search", `by:${who}`, "comment", "date") }, "All →")), h("ul", { class: "hn-res" }, j.recent.map((c) => commentItem(app, { ...c, author: who }, null)))] : null),
      h("aside", {},
        yrs.length > 1 ? h("div", { class: "hn-card" }, h("h4", {}, "Comments a year"), sparkYears(yrs)) : null,
        j.replies_to?.length ? h("div", { class: "hn-card" }, h("h4", {}, "Answers most"), h("div", { class: "hn-ppl" }, j.replies_to.map((r) => h("a", { href: app.href("user", r.who), title: `${r.w} replies` }, ava(r.who), r.who)))) : null,
        j.replied_by?.length ? h("div", { class: "hn-card" }, h("h4", {}, "Answered most by"), h("div", { class: "hn-ppl" }, j.replied_by.map((r) => h("a", { href: app.href("user", r.who), title: `${r.w} replies` }, ava(r.who), r.who)))) : null,
        j.sites?.length ? h("div", { class: "hn-card" }, h("h4", {}, "Sites they post"), h("ul", { class: "hn-mini" }, j.sites.map((r) => h("li", {}, h("a", { href: app.href("search", `by:${who} site:${r.site}`) }, r.site), h("span", { class: "m" }, `${r.n} stories · ${compact(r.points)} points`))))) : null)));
}

// A radial picture of someone's orbit: them in the middle, the people the
// PageRank from them ranks highest around, closer the higher, and the
// replies between them as lines.
function orbit(app, who, j) {
  const W = 640, H = 380, cx = W / 2, cy = H / 2;
  const ring = (j.orbit || []).slice(0, 12);
  if (!ring.length) return h("div", { style: { fontSize: "12.5px", color: "var(--muted)" } }, "Nobody in their orbit yet: the reply graph is built from the comments loaded.");
  const w = new Map();
  for (const r of j.replies_to || []) w.set(r.who, (w.get(r.who) || 0) + r.w);
  for (const r of j.replied_by || []) w.set(r.who, (w.get(r.who) || 0) + r.w);
  const max = Math.max(...ring.map((r) => r.score || 0), 1e-9);
  const svg = h("svg", { class: "hn-graph", viewBox: `0 0 ${W} ${H}` });
  const pos = ring.map((r, i) => {
    const a = (i / ring.length) * Math.PI * 2 - Math.PI / 2;
    const d = 90 + (1 - (r.score || 0) / max) * 80;
    return [cx + Math.cos(a) * d * 1.45, cy + Math.sin(a) * d];
  });
  ring.forEach((r, i) => svg.append(h("line", { x1: cx, y1: cy, x2: pos[i][0], y2: pos[i][1], "stroke-width": 1 + Math.min(5, (w.get(r.id) || 0) / 2), opacity: w.get(r.id) ? 0.9 : 0.35 })));
  svg.append(h("g", {}, h("circle", { cx, cy, r: 26, fill: hue(who), stroke: "var(--surface)", "stroke-width": 3 }), h("text", { x: cx, y: cy + 4, "text-anchor": "middle", style: { fill: "#fff", fontWeight: 700, fontSize: "13px" } }, who.slice(0, 10))));
  ring.forEach((r, i) => {
    const rad = 9 + Math.min(14, Math.sqrt(r.comments || 1) / 6);
    const g = h("g", { class: "nd", onclick: () => app.go("user", r.id) }, h("circle", { cx: pos[i][0], cy: pos[i][1], r: rad, fill: hue(r.id), stroke: "var(--surface)", "stroke-width": 2 }),
      h("text", { x: pos[i][0], y: pos[i][1] + rad + 13, "text-anchor": "middle" }, r.id));
    g.append(h("title", {}, `${r.id}: ${fmt(r.comments)} comments${w.get(r.id) ? `, ${w.get(r.id)} replies with ${who}` : ""}`));
    svg.append(g);
  });
  return svg;
}

// ---- who knows a topic -----------------------------------------------------

async function experts(main, app, topic, alive) {
  const box = h("input", { value: topic, placeholder: "A topic: kubernetes, sqlite, compilers, fundraising…" });
  const go = () => app.go("people", box.value.trim());
  box.onkeydown = (e) => { if (e.key === "Enter") go(); };
  const out = h("div", {}, topic ? skel(4) : h("div", { class: "hn-empty" }, "Name a topic to see who writes the comments people come back to, and how they talk to each other."));
  fill(main, h("h2", { style: { margin: "10px 0 2px", letterSpacing: "-.02em" } }, "Who knows about it"), h("div", { class: "hn-input" }, box, h("button", { class: "hn-btn", onclick: go }, "Find people")), out);
  if (!topic) return;
  const j = await app.api(`/v1/experts?q=${enc(topic)}`);
  if (!alive()) return;
  const ex = j.experts || [];
  if (!ex.length) { fill(out, h("div", { class: "hn-empty" }, "No one stands out on this yet.")); return; }
  fill(out, h("div", { class: "hn-grid two" },
    h("section", {}, h("div", { class: "hn-card" }, h("h4", {}, "How they reply to each other", h("span", { style: { textTransform: "none", letterSpacing: 0, fontWeight: 400 } }, `${fmt(j.read)} comments read`)), network(app, ex, j.edges || [])),
      h("ul", { class: "hn-res" }, ex.slice(0, 14).map((e, i) => h("li", { class: "hn-s" },
        h("div", { class: "hn-pts" }, h("b", {}, i + 1)),
        h("div", {}, h("div", { class: "hn-ttl" }, h("a", { href: app.href("user", e.id) }, ava(e.id), " ", e.id)),
          h("div", { class: "hn-meta" }, h("span", {}, `${e.comments_on_topic} comments on it`), h("span", {}, `in ${e.threads} threads`), e.profile?.first ? h("span", {}, `on HN since ${new Date(e.profile.first).getFullYear()}`) : null, h("a", { href: app.href("search", `${topic} by:${e.id}`, "comment") }, "their comments →")),
          h("div", { class: "hn-again" }, "Best in: ", e.best.map((b, k) => [k ? " · " : "", h("a", { href: app.href("item", b.id) }, b.story_title)]))))))),
    h("aside", {}, j.community?.length ? h("div", { class: "hn-card" }, h("h4", {}, "The community around them"), h("div", { class: "hn-ppl" }, j.community.map((r) => h("a", { href: app.href("user", r.id) }, ava(r.id), r.id))),
      h("div", { style: { fontSize: "12px", color: "var(--muted)", marginTop: "8px" } }, "PageRank over the reply graph, from the people above.")) : null)));
}

// A small force layout of who replies to whom among the people found.
function network(app, people, edges) {
  const W = 680, H = 360;
  const ids = people.slice(0, 18).map((p) => p.id);
  const idx = new Map(ids.map((id, i) => [id, i]));
  const es = edges.filter((e) => idx.has(e.src) && idx.has(e.dst) && e.src !== e.dst);
  const n = ids.map((id, i) => ({ id, x: W / 2 + Math.cos(i * 2.4) * 120 * Math.sqrt(i / ids.length + 0.1), y: H / 2 + Math.sin(i * 2.4) * 100 * Math.sqrt(i / ids.length + 0.1), vx: 0, vy: 0, r: 8 + Math.max(0, 12 - i * 0.6) }));
  for (let it = 0; it < 260; it++) {
    for (let a = 0; a < n.length; a++) for (let b = a + 1; b < n.length; b++) {
      const dx = n[b].x - n[a].x, dy = n[b].y - n[a].y, d2 = Math.max(80, dx * dx + dy * dy), f = 2600 / d2;
      n[a].vx -= dx * f / 60; n[a].vy -= dy * f / 60; n[b].vx += dx * f / 60; n[b].vy += dy * f / 60;
    }
    for (const e of es) {
      const a = n[idx.get(e.src)], b = n[idx.get(e.dst)], dx = b.x - a.x, dy = b.y - a.y, d = Math.sqrt(dx * dx + dy * dy) || 1, f = (d - 90) * 0.004 * Math.min(4, 1 + Math.log(1 + e.w));
      a.vx += dx / d * f * d / 10; a.vy += dy / d * f * d / 10; b.vx -= dx / d * f * d / 10; b.vy -= dy / d * f * d / 10;
    }
    for (const p of n) { p.vx += (W / 2 - p.x) * 0.004; p.vy += (H / 2 - p.y) * 0.006; p.x += p.vx; p.y += p.vy; p.vx *= 0.6; p.vy *= 0.6; p.x = Math.max(30, Math.min(W - 30, p.x)); p.y = Math.max(24, Math.min(H - 28, p.y)); }
  }
  const svg = h("svg", { class: "hn-graph", viewBox: `0 0 ${W} ${H}`, style: { height: "360px" } });
  for (const e of es) { const a = n[idx.get(e.src)], b = n[idx.get(e.dst)]; svg.append(h("line", { x1: a.x, y1: a.y, x2: b.x, y2: b.y, "stroke-width": Math.min(5, 0.8 + Math.log(1 + e.w)), opacity: 0.75 })); }
  for (const p of n) {
    const g = h("g", { class: "nd", onclick: () => app.go("user", p.id) }, h("circle", { cx: p.x, cy: p.y, r: p.r, fill: hue(p.id), stroke: "var(--surface)", "stroke-width": 2 }), h("text", { x: p.x, y: p.y + p.r + 12, "text-anchor": "middle" }, p.id));
    svg.append(g);
  }
  if (!es.length) svg.append(h("text", { x: W / 2, y: H - 8, "text-anchor": "middle" }, "They have not replied to each other in the comments loaded."));
  return svg;
}

// ---- trends ----------------------------------------------------------------

async function trends(main, app, terms, alive) {
  const box = h("input", { value: terms, placeholder: "Terms, separated by commas: rust, go, zig" });
  const go = () => app.go("trends", box.value.trim());
  box.onkeydown = (e) => { if (e.key === "Enter") go(); };
  const out = h("div", {}, terms ? skel(3) : null);
  fill(main, h("h2", { style: { margin: "10px 0 2px", letterSpacing: "-.02em" } }, "Trends"), h("div", { class: "hn-input" }, box, h("button", { class: "hn-btn", onclick: go }, "Compare")), out);
  if (!terms) { trending(out, app, alive); return; }
  const j = await app.api(`/v1/trends?q=${enc(terms)}&in=stories`);
  if (!alive()) return;
  const S = (j.series || []).filter((s) => s.points.length);
  if (!S.length) { fill(out, h("div", { class: "hn-empty" }, "Nothing matched these terms.")); return; }
  const W = 860, H = 300, L = 44, B = 26;
  const ys = S.flatMap((s) => s.points.map((p) => Number(p.p))).filter((y) => y >= 2006);
  const x0 = Math.min(...ys), x1 = Math.max(...ys, x0 + 1);
  const max = Math.max(...S.flatMap((s) => s.points.filter((p) => Number(p.p) >= 2006).map((p) => p.per10k)), 1e-9);
  const X = (y) => L + ((y - x0) / (x1 - x0)) * (W - L - 12), Y = (v) => H - B - (v / max) * (H - B - 14);
  const svg = h("svg", { class: "hn-chart", viewBox: `0 0 ${W} ${H}` });
  for (let k = 0; k <= 4; k++) { const v = (max * k) / 4; svg.append(h("line", { x1: L, x2: W - 12, y1: Y(v), y2: Y(v), stroke: "var(--grid)" }), h("text", { x: L - 6, y: Y(v) + 4, "text-anchor": "end" }, v >= 10 ? Math.round(v) : v.toFixed(1))); }
  for (let y = x0; y <= x1; y += Math.max(1, Math.round((x1 - x0) / 10))) svg.append(h("text", { x: X(y), y: H - 8, "text-anchor": "middle" }, y));
  S.forEach((s, i) => {
    const pts = s.points.filter((p) => Number(p.p) >= 2006).map((p) => [X(Number(p.p)), Y(p.per10k)]);
    svg.append(h("polyline", { points: pts.map((p) => p.join(",")).join(" "), fill: "none", stroke: COLORS[i], "stroke-width": 2.4, "stroke-linejoin": "round", "stroke-linecap": "round" }));
    for (const p of pts) svg.append(h("circle", { cx: p[0], cy: p[1], r: 2.6, fill: COLORS[i] }));
  });
  fill(out, 
    h("div", { class: "hn-card" }, h("h4", {}, "Stories per 10,000, by year"), h("div", { class: "hn-legend" }, S.map((s, i) => h("span", {}, h("i", { style: { background: COLORS[i] } }), s.term, h("small", { style: { color: "var(--muted)" } }, ` ${fmt(s.total)}`)))), svg),
    h("div", { class: "hn-cols" }, S.map((s, i) => h("div", { class: "hn-card" }, h("h4", {}, h("span", { style: { color: COLORS[i] } }, s.term), h("a", { href: app.href("search", s.term, "story", "points") }, "Top stories →")),
      h("ul", { class: "hn-mini" }, (s.top || []).slice(0, 5).map((r) => h("li", {}, h("a", { href: app.href("item", r.id) }, r.title), h("span", { class: "m" }, `${fmt(r.points)} points · ${new Date(r.created).getFullYear()}`))))))));
}

// What is trending, found by the counts alone: the terms of the newest
// titles against the year before, and every month's spikes since 2007.
function trendRows(app, terms, n) {
  return h("ol", { class: "hn-trend" }, (terms || []).slice(0, n).map((t, i) => h("li", {},
    h("span", { class: "rk" }, i + 1),
    h("div", {},
      h("div", { class: "tm" }, h("a", { href: app.href("search", t.term, "", "date") }, t.term), h("span", { class: "x", title: `${t.n} titles where ${t.expected} were usual` }, `×${t.ratio >= 100 ? Math.round(t.ratio) : t.ratio}`), h("span", { class: "m" }, `${fmt(t.n)} stories · ${fmt(t.points)} points`), h("a", { class: "m", href: app.href("trends", t.term) }, "over the years →")),
      t.top?.title ? h("a", { class: "st", href: app.href("item", t.top.id) }, t.top.title) : null))));
}

async function trending(out, app, alive) {
  const now = h("div", {}, skel(3));
  const tabs = h("div", { class: "hn-seg" });
  let hours = 48;
  const drawNow = async () => {
    fill(tabs, [[48, "Last 48 hours"], [168, "This week"], [720, "This month"]].map(([v, t]) => h("button", { class: hours === v ? "on" : "", onclick: () => { hours = v; drawNow(); } }, t)));
    fill(now, skel(3));
    const j = app.add(await app.ctx.api(`/v1/trending?hours=${hours}`).catch(() => null), `trending, ${hours} h`);
    if (!alive()) return;
    if (!j?.terms?.length) { fill(now, h("div", { class: "hn-empty" }, j?.stories ? `Nothing stands out in the last ${hours} hours' ${fmt(j.stories)} stories yet.` : "No stories that new are loaded: the app's --feed keeps them coming.")); return; }
    fill(now, h("div", { class: "hn-sub" }, `${fmt(j.stories)} stories in the window, against ${fmt(j.baseline.stories)} over the ${j.baseline.months} months from ${j.baseline.from}. ×N is how many times its usual share of titles; ranked by Dunning's log-likelihood, so a jump many titles show beats one rare word.`), trendRows(app, j.terms, 20));
  };
  const years = h("div", { class: "hn-seg", style: { flexWrap: "wrap" } });
  const months = h("div", {}, skel(2));
  const thisYear = new Date().getFullYear();
  let year = thisYear - 1;
  const drawYear = async () => {
    const ys = [];
    for (let y = thisYear; y >= 2007; y--) ys.push(y);
    fill(years, ys.map((y) => h("button", { class: y === year ? "on" : "", onclick: () => { year = y; drawYear(); } }, y)));
    fill(months, skel(2));
    const j = app.add(await app.ctx.api(`/v1/spikes?year=${year}`).catch(() => null), `spikes of ${year}`);
    if (!alive()) return;
    const by = new Map();
    for (const r of j?.spikes || []) { if (!by.has(r.month)) by.set(r.month, []); by.get(r.month).push(r); }
    if (!by.size) { fill(months, h("div", { class: "hn-empty" }, "No spikes for this year: the trending rows are made by chloedb-hn trending and loaded with the rest.")); return; }
    const MN = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    fill(months, h("div", { class: "hn-months" }, [...by.entries()].sort().reverse().map(([m, rs]) => h("div", { class: "hn-card" },
      h("h4", {}, `${MN[Number(m.slice(5, 7)) - 1]} ${m.slice(0, 4)}`),
      rs[0]?.top_title ? h("a", { class: "lead", href: app.href("item", rs[0].top) }, rs[0].top_title) : null,
      h("div", { class: "chips" }, rs.slice(0, 8).map((r) => h("a", { class: "hn-chip", href: app.href("search", `${r.term} ${m.slice(0, 4)}`), title: `${r.n} titles where ${r.expected} were usual (×${r.ratio}), ${fmt(r.points)} points` }, r.term, h("small", {}, ` ×${r.ratio >= 100 ? Math.round(r.ratio) : r.ratio}`))))))));
  };
  fill(out,
    h("div", { class: "hn-head", style: { margin: "14px 0 6px" } }, h("h3", { style: { margin: 0 } }, "Trending now"), h("span", { class: "sp" }), tabs),
    now,
    h("div", { class: "hn-head", style: { margin: "26px 0 6px" } }, h("h3", { style: { margin: 0 } }, "Spikes through the years"), h("span", { class: "sp" })),
    h("div", { class: "hn-sub" }, "Each month's titles against the twelve months before: what HN was suddenly about."),
    years, months,
    h("div", { class: "hn-sub", style: { marginTop: "18px" } }, "Or compare terms over the years: type them above, separated by commas."));
  drawNow();
  drawYear();
}

// ---- what HN thinks --------------------------------------------------------

// The model's answer, as it is made: the comments it reads show at once
// (packed by the database), the answer writes in when the model is done,
// with an animation meanwhile. Used by the "What HN thinks" page and inline
// in the results of a question about opinions.
function answerBox(app, question, alive, opts = {}) {
  const cards = new Map();
  const src = new Map();
  const state = h("div", { class: "hn-think" },
    h("span", { class: "orb", html: SPARK }),
    h("div", {}, h("b", {}, "Reading the comments"), h("span", { class: "st" }, "packing the ones that speak to it into the model's budget…")),
    h("span", { class: "dots" }, h("i", {}), h("i", {}), h("i", {})));
  const answer = h("div", { class: "hn-card" }, h("div", { class: "hn-ans" }, opts.title ? h("div", { class: "ey" }, opts.title) : null, state));
  const read = h("div", {});
  const box = h("div", {}, answer, read);
  const cite = (text) => {
    const parts = [];
    let last = 0;
    for (const m of text.matchAll(/\[(\d+)\]/g)) {
      parts.push(text.slice(last, m.index));
      const n = Number(m[1]);
      parts.push(src.has(n) ? h("sup", {}, h("a", { href: app.href("item", src.get(n).id), onmouseenter: () => cards.get(n)?.classList.add("hot"), onmouseleave: () => cards.get(n)?.classList.remove("hot"), title: `${src.get(n).author}, ${src.get(n).year}` }, n)) : `[${n}]`);
      last = m.index + m[0].length;
    }
    parts.push(text.slice(last));
    return parts;
  };
  const showSources = (list) => {
    for (const x of list || []) src.set(x.n, x);
    const shown = (list || []).slice(0, opts.compact ? 4 : 99);
    fill(read,
      h("div", { class: "hn-sec" }, h("h3", {}, opts.compact ? `The comments it reads (${(list || []).length})` : "The comments it read"), h("a", { href: app.href("search", question.replace(/^what (does|do) (hn|people) think (about|of)\s*/i, "").replace(/\?$/, ""), "comment") }, "Search them →")),
      h("div", { class: "hn-src" }, shown.map((s) => { const c = h("div", { id: `s${s.n}` }, h("div", {}, h("span", { class: "n" }, s.n), h("a", { href: app.href("user", s.author), style: { fontWeight: 600 } }, s.author), ` · ${s.year} · `, h("a", { href: app.href("item", s.id), style: { color: "var(--muted)" } }, s.story_title)), h("div", { class: "x" }, s.text)); cards.set(s.n, c); return c; })));
  };
  const showAnswer = (j) => {
    if (!j.answer) { fill(answer, h("div", { class: "hn-empty" }, j.note || "No answer.")); return; }
    if (j.sources) showSources(j.sources);
    const blocks = [];
    let ul = null;
    for (const line of j.answer.split("\n")) {
      const t = line.trim();
      if (!t) { ul = null; continue; }
      const b = t.match(/^[-*•]\s+(.*)/);
      if (b) { if (!ul) { ul = h("ul", {}); blocks.push(ul); } ul.append(h("li", {}, cite(b[1].replace(/\*\*(.+?)\*\*/g, "$1")))); }
      else { ul = null; blocks.push(h("p", {}, cite(t.replace(/\*\*(.+?)\*\*/g, "$1")))); }
    }
    const r = j.receipt;
    fill(answer, h("div", { class: "hn-ans hn-in" }, opts.title ? h("div", { class: "ey" }, opts.title) : null, blocks),
      h("div", { class: "hn-receipt" },
        h("span", {}, `${(j.sources || []).length} comments cited`),
        r ? h("span", {}, `${r.skipped} more left out by the pack (near duplicates, over budget)`) : null,
        h("span", {}, j.kept ? `kept answer from ${day(j.at)}` : `${(j.ms / 1000).toFixed(1)} s`),
        h("span", {}, j.model),
        j.kept && !opts.compact ? h("a", { href: "#", style: { color: "var(--accent)" }, onclick: (e) => { e.preventDefault(); run(true); } }, "Ask again") : null));
  };
  const fail = (e) => fill(answer, h("div", { class: "hn-empty" }, String(e?.message || e || "The model did not answer.")));
  async function run(fresh) {
    if (fresh) fill(answer, h("div", { class: "hn-ans" }, state));
    // The comments first (fast, and the whole answer when it is kept).
    const k = fresh ? null : app.add(await app.ctx.api(`/v1/ask/comments?q=${enc(question)}`).catch(() => null), "comments read");
    if (!alive()) return;
    if (k?.answer) { showAnswer(k); return; }
    if (k && !k.pending) { fill(answer, h("div", { class: "hn-empty" }, "No comments speak to this yet.")); read.replaceChildren(); return; }
    if (k) {
      showSources(k.sources);
      fill(state.querySelector("b"), "Writing the answer");
      fill(state.querySelector(".st"), `from ${k.sources.length} comments, each claim cited; usually 5 to 15 seconds`);
    }
    try {
      const j = app.add(await app.ctx.api(`/v1/ask?q=${enc(question)}${fresh ? "&fresh=1" : ""}`, { timeout: 120000 }), "the answer");
      if (alive()) showAnswer(j);
    } catch (e) { if (alive()) fail(e); }
  }
  run(false);
  return box;
}

async function ask(main, app, question, alive) {
  const box = h("input", { value: question, placeholder: "What does HN think about…" });
  const go = () => app.go("ask", box.value.trim());
  box.onkeydown = (e) => { if (e.key === "Enter") go(); };
  const out = h("div", {}, question ? null : h("div", { class: "hn-empty" }, "Ask a question; the answer comes from HN's comments, each claim cited to the comment it came from."));
  fill(main, h("h2", { style: { margin: "10px 0 2px", letterSpacing: "-.02em" } }, "What HN thinks"), h("div", { class: "hn-input" }, box, h("button", { class: "hn-btn", onclick: go }, "Ask")), out);
  if (!question) return;
  app.queries = [];
  fill(out, answerBox(app, question, alive));
}

// ---- the front page --------------------------------------------------------

async function front(main, app, alive) {
  fill(main, skel(6));
  const j = await app.api("/v1/front");
  if (!alive()) return;
  const f = j.feed;
  fill(main, 
    h("div", { class: "hn-head", style: { margin: "10px 0" } }, h("h2", { style: { margin: 0, letterSpacing: "-.02em" } }, "The front page, from the tables"),
      h("span", { class: "t" }, f?.running ? ["read from the HN API every half minute · ", fmt(f.written), " items written"] : "the newest stories loaded, ranked as HN ranks them: points over age")),
    (j.stories || []).length ? h("ul", { class: "hn-res" }, j.stories.map((s) => storyItem(app, s, null))) : h("div", { class: "hn-empty" }, "No stories from the last two days are loaded: start the app with --feed to keep it live."));
}

// ---- how it works ----------------------------------------------------------

function about(main, app) {
  const p = (t) => h("p", { style: { color: "var(--text-2)", maxWidth: "760px" } }, t);
  fill(main, h("div", { style: { maxWidth: "820px" } },
    h("h2", { style: { letterSpacing: "-.02em" } }, "How this works"),
    p("Every Hacker News story and comment is a row of a ChloeDB table, with its text indexed for words (BM25, with typo tolerance and HN's synonyms: k8s, postgres, LLM…) and for meaning (an embedding of each title and comment, made by the database as the rows were written). People are rows; who replied to whom is an edge table, and so is which discussion links to which."),
    p("A search is understood first: by:, site:, after:, type:, points> and quoted phrases, or the same said as a sentence (“Show HN posts about databases since 2021 with over 300 points”). Stories are found by their words and by meaning, the two lists fused and ranked by relevance, points and age; a comment is shown in its story, with the way to it opened in the thread. The facets beside the results are GROUP BY queries over the same match."),
    p("What HN thinks: the comments that speak to a question are found by words and by meaning, packed into a language model's budget by the database (near duplicates skipped, long comments cut to their best stretch), and summed up by the model through ChloeDB's llm(), each claim cited to a numbered comment."),
    p("Who knows a topic ranks the people whose comments on it rank best, across many threads; their orbit is a personalized PageRank over the reply graph. Trends count the matches per year as a share of everything posted that year."),
    p("The SQL button at the bottom right shows every query a page ran, with its rows and time."),
    h("p", { style: { color: "var(--muted)", fontSize: "13px" } }, "Data: the open-index/hacker-news archive on Hugging Face (ODC-By), kept up to date from the official HN API. Not affiliated with Y Combinator or Hacker News.")));
}

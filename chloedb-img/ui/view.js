// Image Explorer's page: the view the command center shows for the app
// `img` (chloedb-server --app img=URL), and the page the app serves at /
// with --public. One module, no build step: render(el, ctx) draws into
// `el`, and ctx = { api, path, href, onPath } (and, on the page of its own,
// img(id, size), the address of a thumbnail).
//
// Every search is one SQL statement the app runs on ChloeDB; each answer
// carries them as `queries`, shown by the footer's "The SQL behind this
// page".

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
@media (max-width: 780px) { .ax-grid, .ax-grid.two { grid-template-columns: minmax(0, 1fr); } .ax-grid > .ax-left { display: none; } .ax-brand small { display: none; } .ax-box .kbd { display: none; } .ax-box input { font-size: 16px; } .ax-try.ax-row { flex-wrap: nowrap; overflow-x: auto; scrollbar-width: none; padding-bottom: 2px; } .ax-try.ax-row > * { flex: none; } }
.ax-left { position: sticky; top: 108px; max-height: calc(100vh - 120px); overflow: auto; scrollbar-width: thin; padding-right: 4px; }
.ax-facet { margin-bottom: 18px; }
.ax-facet h4 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; }
.ax-facet .r { display: flex; align-items: center; gap: 8px; padding: 3px 6px; margin: 0 -6px; border-radius: 6px; cursor: pointer; font-size: 13px; color: var(--text-2); }
.ax-facet .r:hover { background: var(--surface-2); color: var(--text); }
.ax-facet .r .n { margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; }
.ax-facet .r .l { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ax-head { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 4px; }
.ax-head .t { font-size: 13px; color: var(--muted); }
.ax-head .t b { color: var(--text); font-weight: 600; }
.ax-head .sp { flex: 1; }
.ax-seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 8px; overflow: hidden; background: var(--surface); }
.ax-seg button { font: 12.5px var(--sans); border: 0; background: none; color: var(--text-2); padding: 4px 10px; cursor: pointer; }
.ax-seg button + button { border-left: 1px solid var(--line); }
.ax-seg button.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.ax-sec { display: flex; align-items: baseline; justify-content: space-between; margin: 24px 0 6px; gap: 10px; }
.ax-sec h3 { margin: 0; font-size: 12.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
.ax-sec a, .ax-sec .a { font-size: 13px; color: var(--accent); cursor: pointer; }
.ax-sec .note { font-size: 12px; color: var(--muted); }
.ax-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; box-shadow: var(--shadow-1); }
.ax-card h4 { margin: 0 0 8px; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; display: flex; justify-content: space-between; gap: 8px; }
.ax-card h4 a { text-transform: none; letter-spacing: 0; color: var(--accent); font-weight: 500; }
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
.ax { --ax-w: 1320px; }
.ix-row { display: flex; flex-wrap: wrap; gap: 6px; }
.ix-row > a, .ix-row > div { position: relative; display: block; height: 168px; border-radius: 8px; overflow: hidden; background: var(--surface-3); }
.ix-row.lg > a, .ix-row.lg > div { height: 210px; }
.ix-row.sm > a, .ix-row.sm > div { height: 118px; }
.ix-row.xs > a, .ix-row.xs > div { height: 84px; border-radius: 6px; }
.ix-row img { width: 100%; height: 100%; object-fit: cover; display: block; }
.ix-row > a:hover { text-decoration: none; outline: 2px solid var(--accent); outline-offset: 1px; }
.ix-row .cap { position: absolute; left: 0; right: 0; bottom: 0; padding: 18px 8px 6px; font-size: 11.5px; line-height: 1.3; color: #fff; background: linear-gradient(180deg, transparent, rgba(0, 0, 0, .66)); display: flex; gap: 6px; align-items: flex-end; }
.ix-row .cap .s { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ix-row .cap .lic { margin-left: auto; flex: none; font-size: 10px; font-weight: 650; letter-spacing: .04em; background: rgba(255, 255, 255, .22); border-radius: 4px; padding: 0 4px; }
.ix-row .vote { position: absolute; top: 6px; right: 6px; display: flex; gap: 4px; }
.ix-row .vote span { width: 24px; height: 24px; border-radius: 6px; background: rgba(15, 18, 22, .62); color: #fff; display: inline-flex; align-items: center; justify-content: center; font-size: 13px; line-height: 1; }
.ix-row .vote span.up { background: var(--good); }
.ix-row .vote span.down { background: var(--crit); }
.ix-row > .liked { outline: 3px solid var(--good); outline-offset: 1px; }
.ix-row > .disliked { opacity: .38; }
.ix-row .rank { position: absolute; top: 6px; left: 6px; font: 600 11px var(--sans); color: #fff; background: rgba(15, 18, 22, .62); border-radius: 5px; padding: 1px 6px; }
.ix-ex { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin: 20px auto 6px; max-width: 980px; text-align: left; }
.ix-ex a { display: flex; gap: 10px; align-items: center; padding: 8px 10px; border-radius: 11px; border: 1px solid var(--line-2); background: var(--surface); }
.ix-ex a:hover { border-color: var(--accent); text-decoration: none; }
.ix-ex img { width: 46px; height: 46px; border-radius: 7px; object-fit: cover; flex: none; }
.ix-ex .k { display: block; font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }
.ix-ex .q { display: block; font-size: 13.5px; color: var(--text); line-height: 1.3; }
.ix-tools { display: flex; gap: 6px; align-items: center; position: absolute; right: 8px; top: 50%; transform: translateY(-50%); }
.ix-tools span { display: inline-flex; align-items: center; gap: 5px; font-size: 12px; color: var(--text-2); border: 1px solid var(--line-2); border-radius: 7px; padding: 3px 8px; background: var(--surface-2); }
.ix-tools svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.ix-sw { display: inline-block; width: 16px; height: 16px; border-radius: 4px; vertical-align: -3px; border: 1px solid rgba(0, 0, 0, .08); }
.ix-pal { display: flex; height: 26px; border-radius: 7px; overflow: hidden; border: 1px solid var(--line); }
.ix-pal span { flex: 1; }
.ix-pal.sm { height: 8px; border-radius: 3px; border: 0; margin-top: 4px; }
.ix-hero-img { width: 100%; border-radius: 10px; display: block; background: var(--surface-3); }
.ix-credit { font-size: 13px; color: var(--text-2); }
.ix-credit .line { font: 12px var(--mono); background: var(--code); border: 1px solid var(--line); border-radius: 7px; padding: 8px 10px; margin: 8px 0; color: var(--text); white-space: normal; }
.ix-kv { display: grid; grid-template-columns: 96px minmax(0, 1fr); gap: 4px 10px; font-size: 13px; }
.ix-kv dt { color: var(--muted); }
.ix-kv dd { margin: 0; color: var(--text); min-width: 0; overflow-wrap: anywhere; }
.ix-box { position: absolute; border: 2px solid #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, .5), 0 0 0 9999px rgba(0, 0, 0, .35); border-radius: 4px; }
.ix-box .h { position: absolute; width: 9px; height: 9px; background: #fff; border-radius: 2px; box-shadow: 0 0 0 1px rgba(0, 0, 0, .4); }
.ix-path { display: flex; align-items: center; gap: 0; overflow: hidden; }
.ix-path .st { flex: 1; min-width: 0; text-align: center; }
.ix-path .st img { width: 100%; height: 132px; object-fit: cover; border-radius: 8px; display: block; }
.ix-path .st .l { font-size: 11.5px; color: var(--muted); margin-top: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; padding: 0 2px; }
.ix-path .ar { flex: none; width: 18px; text-align: center; color: var(--muted); font-size: 13px; }
.ix-path .end img { outline: 3px solid var(--accent); outline-offset: 1px; }
.ix-mix { display: grid; grid-template-columns: 180px 26px 1fr 26px 1fr; gap: 10px; align-items: center; }
.ix-mix .op { font-size: 22px; color: var(--muted); text-align: center; }
.ix-mix img { width: 100%; height: 120px; object-fit: cover; border-radius: 8px; display: block; }
.ix-mix .w { background: var(--surface); border: 1px solid var(--line-2); border-radius: 10px; padding: 10px 12px; font-size: 15px; }
.ix-mix .w small { display: block; font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: .05em; }
.ix-slider { display: flex; align-items: center; gap: 10px; font-size: 12.5px; color: var(--muted); margin-top: 10px; }
.ix-slider .tr { flex: 1; height: 4px; border-radius: 2px; background: var(--surface-3); position: relative; }
.ix-slider .tr i { position: absolute; top: -6px; width: 16px; height: 16px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 3px var(--surface); }
.ix-slider .tr b { position: absolute; left: 0; top: 0; height: 100%; background: var(--accent); border-radius: 2px; }
.ix-atlas { position: relative; height: 820px; border-radius: 12px; background: radial-gradient(ellipse at 50% 45%, var(--surface) 0%, var(--sunken) 100%); border: 1px solid var(--line); overflow: hidden; }
.ix-atlas .cl { position: absolute; transform: translate(-50%, -50%); text-align: center; }
.ix-atlas .cl .tiles { display: grid; gap: 2px; border-radius: 8px; overflow: hidden; box-shadow: var(--shadow-2); background: var(--surface); }
.ix-atlas .cl img { width: 100%; height: 100%; object-fit: cover; display: block; }
.ix-atlas .cl .nm { margin-top: 5px; font-size: 12.5px; font-weight: 640; color: var(--text); background: color-mix(in srgb, var(--surface) 88%, transparent); border-radius: 6px; padding: 1px 7px; display: inline-block; white-space: nowrap; }
.ix-atlas .cl .n { display: block; font-size: 11px; color: var(--muted); font-weight: 500; }
.ix-atlas .zoom { position: absolute; right: 14px; bottom: 14px; display: flex; flex-direction: column; gap: 4px; }
.ix-atlas .zoom span { width: 32px; height: 32px; border-radius: 8px; background: var(--surface); border: 1px solid var(--line-2); display: inline-flex; align-items: center; justify-content: center; font-size: 17px; color: var(--text-2); }
.ix-atlas .lab { position: absolute; left: 14px; top: 12px; font-size: 12.5px; color: var(--muted); background: var(--surface); border: 1px solid var(--line); border-radius: 8px; padding: 6px 10px; max-width: 420px; }
.ix-picker { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.ix-picker .c { width: 44px; height: 44px; border-radius: 10px; border: 2px solid var(--surface); box-shadow: 0 0 0 1px var(--line-2); display: inline-block; }
.ix-picker .c.on { box-shadow: 0 0 0 2px var(--accent); }
.ix-picker .add { width: 44px; height: 44px; border-radius: 10px; border: 1px dashed var(--line-2); display: inline-flex; align-items: center; justify-content: center; color: var(--muted); font-size: 20px; }
.ix-wheel { display: grid; grid-template-columns: repeat(12, 1fr); gap: 3px; }
.ix-wheel span { aspect-ratio: 1; border-radius: 4px; display: block; }
.ix-drop { border: 1.5px dashed var(--line-2); border-radius: 12px; padding: 18px; text-align: center; color: var(--text-2); font-size: 13.5px; background: var(--surface); }
.ix-drop b { color: var(--accent); }
.ix-tag { display: inline-flex; align-items: center; gap: 5px; font-size: 12px; padding: 1px 8px; border-radius: 999px; background: var(--surface-3); color: var(--text-2); }
.ix-tag.pd { background: var(--good-soft); color: var(--good-ink); }
.ix-two { display: grid; grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr); gap: 28px; align-items: start; }
.ix-note { font-size: 12.5px; color: var(--muted); margin: 6px 0 0; }
.ix-dup { display: flex; gap: 10px; align-items: center; padding: 8px 0; border-top: 1px solid var(--line); font-size: 13px; }
.ix-dup:first-child { border-top: 0; }
.ix-dup img { width: 56px; height: 56px; border-radius: 6px; object-fit: cover; flex: none; }
.ix-dup .m { color: var(--muted); font-size: 12px; }
.ix-board { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; border-radius: 10px; overflow: hidden; }
.ix-board img { width: 100%; height: 74px; object-fit: cover; display: block; }
.ix-row > div > a { display: block; width: 100%; height: 100%; }
.ix-row .cap { pointer-events: none; }
.ix-row > div:hover { outline: 2px solid var(--accent); outline-offset: 1px; }
.ix-row > i { flex: 999 1 0px; height: 0; }
.ix-row .vote button { width: 30px; height: 30px; padding: 0; border: 0; border-radius: 7px; background: rgba(15, 18, 22, .62); color: #fff; display: inline-flex; align-items: center; justify-content: center; cursor: pointer; }
.ix-row .vote button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
.ix-row .vote button.up.on { background: var(--good); }
.ix-row .vote button.down.on { background: var(--crit); }
.ix-row .pal { position: absolute; left: 6px; right: 6px; bottom: 6px; display: flex; height: 9px; border-radius: 3px; overflow: hidden; box-shadow: 0 0 0 1.5px rgba(255, 255, 255, .7); }
.ix-row .pal span { display: block; height: 100%; }
.ix-ways { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; }
.ix-way { display: block; background: var(--surface); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; box-shadow: var(--shadow-1); color: var(--text); }
.ix-way:hover { border-color: var(--accent); text-decoration: none !important; }
.ix-way .th { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 2px; height: 96px; background: var(--surface-3); }
.ix-way .th img { width: 100%; height: 96px; object-fit: cover; display: block; }
.ix-way .b { padding: 10px 12px 13px; }
.ix-way h3 { margin: 0 0 3px; font-size: 14.5px; font-weight: 650; letter-spacing: -.005em; }
.ix-way p { margin: 0; font-size: 13px; color: var(--text-2); line-height: 1.45; }
.ix-map { position: relative; border-radius: 12px; overflow: hidden; border: 1px solid var(--line); background: #eef0f3; }
.ix-map > img { display: block; width: 100%; height: auto; }
.ix-map .nm { position: absolute; transform: translate(-50%, 0); max-width: 10.4%; font-size: 11.5px; line-height: 1.25; font-weight: 640; color: #0f1216; background: rgba(255, 255, 255, .94); border-radius: 6px; padding: 2px 6px; text-align: center; box-shadow: 0 1px 2px rgba(16, 24, 40, .12); }
.ix-map .nm small { display: block; font-size: 10.5px; color: #555d68; font-weight: 500; }
.ix-map a.nm:hover { text-decoration: none; outline: 2px solid #2f5bd3; }
.ix-map .nm.on { background: #2f5bd3; color: #fff; }
.ix-map .nm.on small { color: #e6ecfb; }
.ix-styles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px 26px; }
.ix-styles h4 { margin: 0 0 6px; font-size: 13px; font-weight: 600; color: var(--text); }
.ix-pic { position: relative; }
.ix-pic img { width: 100%; display: block; border-radius: 10px; }
.ix-steer { display: flex; flex-wrap: wrap; gap: 10px 16px; align-items: center; background: var(--surface); border: 1px solid var(--line); border-radius: 12px; padding: 10px 12px; margin: 8px 0 12px; font-size: 13px; color: var(--text-2); }
.ix-steer img { width: 40px; height: 40px; border-radius: 6px; object-fit: cover; display: block; }
.ix-steer .g { display: flex; gap: 6px; align-items: center; }
.ix-steer .g b { font-size: 11px; text-transform: uppercase; letter-spacing: .05em; color: var(--muted); font-weight: 650; margin-right: 2px; }
.ix-steer .up img { outline: 2px solid var(--good); outline-offset: 1px; }
.ix-steer .dn img { opacity: .5; outline: 2px solid var(--crit); outline-offset: 1px; }
.ix-steer .ix-slider { margin: 0; min-width: 220px; flex: 1; }
.ix-split { display: grid; grid-template-columns: minmax(0, 420px) minmax(0, 1fr); gap: 28px; align-items: start; }
.ix-split h2 { margin: 0 0 4px; font-size: 20px; letter-spacing: -.015em; }
.ix-split .lead { margin: 0 0 12px; font-size: 14px; color: var(--text-2); line-height: 1.5; }
.ix-split h4 { margin: 16px 0 6px; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
@media (max-width: 780px) { .ix-split { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 1000px) { .ix-ways { grid-template-columns: repeat(2, minmax(0, 1fr)); } .ix-styles { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 560px) { .ix-ways { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 780px) { .ix-ex { grid-template-columns: minmax(0, 1fr); } .ix-two { grid-template-columns: minmax(0, 1fr); } .ix-row > a, .ix-row > div { height: 118px; } .ix-tools { display: none; } }

/* What the live page adds to the mock's styles. */
.ix-wait { background: linear-gradient(90deg, var(--surface-2), var(--surface-3), var(--surface-2)); background-size: 200% 100%; animation: axsk 1.2s infinite; border-radius: 8px; }
.ix-row > .ix-wait { height: 168px; }
.ix-row.lg > .ix-wait { height: 210px; }
.ix-row.sm > .ix-wait { height: 118px; }
.ix-gone { display: flex; align-items: center; justify-content: center; min-height: 220px; border-radius: 10px; background: var(--surface-3); color: var(--muted); font-size: 13px; padding: 20px; text-align: center; }
.ix-row img { background: var(--surface-3); }
.ix-row > div .vote { opacity: 0; transition: opacity .12s; }
.ix-row > div:hover .vote, .ix-row > div:focus-within .vote, .ix-row > div.liked .vote { opacity: 1; }
@media (hover: none) { .ix-row > div .vote { opacity: 1; } }
.ix-ex .ph { width: 46px; height: 46px; border-radius: 7px; flex: none; background: var(--surface-3); display: inline-flex; align-items: center; justify-content: center; color: var(--accent); overflow: hidden; }
.ix-ex .ph.sw i { flex: 1; height: 100%; display: block; }
.ix-ex .ph.ic svg { width: 22px; height: 22px; }
.ax-box.drop input { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.ix-pb svg { flex: none; }
.ix-in { width: 100%; font: 14px var(--sans); padding: 8px 12px; border-radius: 9px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; }
.ix-in:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.ix-bare { font: inherit; border: 0; padding: 0; background: none; color: var(--text); width: 100%; outline: none; }
.ix-range { flex: 1; accent-color: var(--accent); min-width: 120px; }
.ix-h1 { margin: 0; font-size: 24px; letter-spacing: -.02em; line-height: 1.2; }
.ix-sub { margin: 4px 0 0; font-size: 14px; color: var(--text-2); }
.ix-eyebrow { font-size: 12px; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; font-weight: 650; }
.ix-eyebrow a { color: var(--accent); }
.ix-of img { width: 64px; height: 64px; border-radius: 8px; object-fit: cover; display: block; }
.ix-chips { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
.ix-chosen { display: flex; gap: 10px; align-items: center; margin: 6px 0; }
.ix-chosen img { width: 72px; height: 72px; border-radius: 8px; object-fit: cover; display: block; outline: 3px solid var(--accent); outline-offset: 1px; }
.ix-big img { max-height: 80vh; width: 100%; object-fit: contain; background: var(--surface-3); min-height: 120px; }
.ix-draw { touch-action: none; user-select: none; -webkit-user-select: none; cursor: crosshair; overflow: hidden; border-radius: 10px; background: var(--surface-3); }
.ix-draw img { pointer-events: none; height: 100%; object-fit: cover; }
.ix-draw .ix-box { cursor: move; }
.ix-box .h.nw { left: -5px; top: -5px; cursor: nwse-resize; }
.ix-box .h.ne { right: -5px; top: -5px; cursor: nesw-resize; }
.ix-box .h.sw { left: -5px; bottom: -5px; cursor: nesw-resize; }
.ix-box .h.se { right: -5px; bottom: -5px; cursor: nwse-resize; }
.ix-drop.on { border-color: var(--accent); background: var(--accent-soft); }
.ix-drop a { color: var(--accent); }
.ix-drop svg { width: 28px; height: 28px; }
.ix-steer .why { flex-basis: 100%; font-size: 12.5px; color: var(--muted); }
.ix-steer a { display: block; }
.ix-dup { color: var(--text); }
.ix-dup:hover { text-decoration: none !important; background: var(--surface-2); }
.ix-path { align-items: flex-start; }
.ix-path .ar { margin-top: 56px; }
.ix-path .st .l { text-align: center; }
.ix-mix > a { display: block; }
.ix-mix .ix-wait { height: 120px; }
.ix-mix .ix-first a { display: block; }
.ix-wmap { display: grid; gap: 22px 16px; padding: 20px; border-radius: 12px; background: var(--sunken); border: 1px solid var(--line); overflow-x: auto; }
.ix-isl { display: block; text-align: center; color: var(--text); min-width: 0; transition: opacity .15s; }
.ix-isl:hover { text-decoration: none !important; }
.ix-isl .tiles { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 2px; border-radius: 9px; overflow: hidden; box-shadow: var(--shadow-1); background: var(--surface-3); aspect-ratio: 1; }
.ix-isl .tiles span { display: block; min-width: 0; aspect-ratio: 1; background: var(--surface-3); }
.ix-isl .tiles img { width: 100%; height: 100%; object-fit: cover; display: block; }
.ix-isl:hover .tiles { outline: 2px solid var(--accent); outline-offset: 2px; }
.ix-isl .nm { display: block; margin-top: 6px; font-size: 12.5px; font-weight: 640; line-height: 1.25; }
.ix-isl .n { display: block; font-size: 11px; color: var(--muted); }
.ix-isl.dim { opacity: .22; }
.ix-zoom { display: inline-flex; gap: 4px; }
.ix-zoom .ax-btn { width: 32px; justify-content: center; padding: 6px 0; }
.ix-wheel button { aspect-ratio: 1; border-radius: 4px; display: block; border: 0; padding: 0; cursor: pointer; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .06); }
.ix-wheel button:hover { outline: 2px solid var(--accent); outline-offset: 1px; }
.ix-picker .c { cursor: pointer; padding: 0; }
.ix-picker .add { position: relative; cursor: pointer; }
.ix-picker .add input { position: absolute; inset: 0; opacity: 0; cursor: pointer; width: 100%; height: 100%; }
.ix-hex { font-family: var(--mono); color: var(--muted); margin-right: 8px; }
.ix-table { overflow-x: auto; border: 1px solid var(--line); border-radius: 10px; }
.ix-table table { border-collapse: collapse; width: 100%; font-size: 13.5px; }
.ix-table th { padding: 8px 12px; background: var(--surface-2); text-align: left; font-weight: 600; }
.ix-table tr + tr, .ix-table tbody tr { border-top: 1px solid var(--line); }
.ix-card-p { margin: 0; font-size: 13px; color: var(--text-2); line-height: 1.55; }
.ix-card-p code { font: 11.5px var(--mono); overflow-wrap: anywhere; }
.ax-grid.two-l { grid-template-columns: 210px minmax(0, 1fr); }
.ix-fbtn { display: none !important; }
.ix-try { display: inline-flex; align-items: center; gap: 12px; padding: 10px 14px 10px 10px; border-radius: 12px; border: 1px solid var(--line-2); background: var(--surface); box-shadow: var(--shadow-1); color: var(--text); margin: 4px 0 6px; max-width: 100%; flex-wrap: wrap; }
.ix-try:hover { border-color: var(--accent); text-decoration: none !important; }
.ix-try img { width: 96px; height: 72px; object-fit: cover; border-radius: 8px; display: block; }
.ix-try .op { font-size: 20px; color: var(--muted); }
.ix-try .w { font-size: 15px; font-weight: 600; }
.ix-try .go { color: var(--accent); font-size: 13.5px; margin-left: 6px; }
.ix-split .ix-row { margin-bottom: 4px; }
@media (max-width: 780px) {
  .ax-grid.two-l { grid-template-columns: minmax(0, 1fr); }
  .ax-grid.two-l > .ax-left { display: block; position: static; max-height: none; display: flex; gap: 6px 18px; flex-wrap: wrap; }
  .ax-grid.two-l > .ax-left .ax-facet { margin-bottom: 6px; min-width: 150px; }
  .ix-pb span { display: none; }
  .ix-pb { padding: 9px 10px; }
  .ix-mix { grid-template-columns: 1fr 18px 1fr; }
  .ix-mix > :nth-child(4), .ix-mix > :nth-child(5) { display: none; }
  .ix-path { overflow-x: auto; }
  .ix-path .st { flex: 0 0 110px; }
  .ix-path .st img { height: 96px; }
  .ix-path .ar { margin-top: 40px; }
  .ix-wmap { padding: 12px; gap: 14px 10px; --isl-min: 112px; }
  .ix-fbtn { display: inline-flex !important; }
  .ax-grid.two-l > .ax-left:not(.open) { display: none; }
  .ax-bar { flex-wrap: wrap; row-gap: 8px; }
  .ax-bar .ax-box { order: 3; flex-basis: 100%; }
  .ax-bar .ix-pb { margin-left: auto; }
  .ax-hero { padding: 26px 0 14px; }
  .ax-hero h1 { font-size: 28px; }
  .ax-hero p { font-size: 14.5px; }
  .ix-ways .th img, .ix-way .th { height: 80px; }
}

`;

// ---------------------------------------------------------------- icons

const ICON = `<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2.5"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5.5-5.5L6 20"/></svg>`;
const SEARCH = `<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>`;
const CAMERA = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>`;
const CORNERS = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5"/></svg>`;
const PLUS = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>`;
const ARROWS = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12h16M15 7l5 5-5 5"/></svg>`;
const UP = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 11v9H4v-9z"/><path d="M7 11l4-7a2.5 2.5 0 0 1 2.5 2.5V10h5a2 2 0 0 1 2 2.3l-1.2 6A2 2 0 0 1 17.3 20H7"/></svg>`;
const HIDE = `<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/><path d="M6 6l12 12"/></svg>`;

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
const enc = encodeURIComponent;
const fmt = (n) => (n == null ? "" : Number(n).toLocaleString("en-US"));
const short = (n) => { n = Number(n) || 0; return n >= 1e6 ? (n / 1e6).toFixed(n >= 1e7 ? 0 : 1).replace(/\.0$/, "") + "M" : n >= 1e4 ? Math.round(n / 1e3) + "k" : n >= 1e3 ? (n / 1e3).toFixed(1) + "k" : String(n); };
const words = (n) => { n = Number(n) || 0; return n >= 1e6 ? `${(n / 1e6).toFixed(1).replace(/\.0$/, "")} million` : fmt(n); };
function skel(n = 5) { return Array.from({ length: n }, (_, i) => h("div", { class: "ax-skel", style: { width: `${60 + ((i * 37) % 40)}%` } })); }
function skelRow(n = 12, size = "") {
  return h("div", { class: `ix-row ${size}` }, Array.from({ length: n }, (_, i) => { const r = [1.4, 0.75, 1.1, 1.6, 0.8, 1.3][i % 6]; return h("div", { class: "ix-wait", style: { flex: `${r} 1 ${Math.round(r * 160)}px` } }); }), h("i"));
}
const LICENCE = { CC0: "CC0", PD: "Public domain" };
const KIND_WORDS = { Photo: "Photo", "Old photo": "Old photo", Painting: "Painting", Drawing: "Drawing", Print: "Print", Map: "Map", Illustration: "Illustration", Object: "Object", "Text page": "Text page", Nature: "Nature photo" };
const SHAPES = { tall: "Tall", square: "Square", wide: "Wide", panorama: "Panorama" };
const SIZES = { small: "Under 1 MP", medium: "1 to 4 MP", large: "4 to 16 MP", huge: "Over 16 MP" };

// Calls f when the element comes near the screen.
function lazy(el, f) {
  if (!("IntersectionObserver" in window)) { f(); return; }
  const io = new IntersectionObserver((es) => { if (es.some((e) => e.isIntersecting)) { io.disconnect(); f(); } }, { rootMargin: "900px" });
  setTimeout(() => io.observe(el), 0);
}

// A picture's thumbnail ("t", 400 px high) or larger copy ("l", up to
// 1,600 px). On the page of its own an <img> loads it from the app; in the
// command center, whose requests carry a key an <img> cannot, it comes as
// JSON when it scrolls near. A picture whose file cannot be read takes
// its tile away (or says so, alone).
function pimg(app, id, size, alt, onload) {
  const im = h("img", { alt: alt || "", decoding: "async" });
  const failed = () => {
    if (im.closest(".ix-ex, .ix-way, .ix-steer, .ix-dup, .ix-of, .ix-chosen, .ix-mix, .ix-isl, .ix-try")) { im.style.visibility = "hidden"; return; }
    const t = im.closest(".ix-row > a, .ix-row > div, .ix-path .st");
    if (t && t.parentNode && !t.classList.contains("keep")) { t.remove(); return; }
    im.replaceWith(h("div", { class: "ix-gone" }, "This picture's file could not be read from the collection."));
  };
  im.addEventListener("error", failed);
  if (onload) im.addEventListener("load", () => onload(im));
  if (app.ctx.img) {
    im.loading = "lazy";
    im.src = app.ctx.img(id, size);
    return im;
  }
  lazy(im, async () => {
    try {
      const j = await app.ctx.api(`/v1/thumb/${enc(id)}${size === "l" ? "?size=large" : ""}`, { timeout: 90000 });
      im.src = `data:${j.mime};base64,${j.data}`;
    } catch { failed(); }
  });
  return im;
}

const HEIGHT = { lg: 210, "": 168, sm: 118, xs: 84 };
function ratio(p) { const r = p.w > 0 && p.h > 0 ? p.w / p.h : 1.2; return Math.max(0.42, Math.min(2.8, r)); }
function flex(p, size = "") { const r = ratio(p); return `${r.toFixed(3)} 1 ${Math.round(r * HEIGHT[size])}px`; }
function cap(p) { return h("span", { class: "cap" }, h("span", { class: "s" }, p.title || ""), p.licence ? h("span", { class: "lic" }, p.licence) : null); }

// A picture in a justified row: its thumbnail, linked to its page (or to `o.href`).
function tile(app, p, o = {}) {
  const a = h("a", { href: o.href || app.href("picture", p.id), style: { flex: flex(p, o.size || "") }, title: p.title || "", onclick: o.onclick });
  a.append(pimg(app, p.id, "t", p.title), o.cap === false ? null : cap(p));
  if (o.pal && Array.isArray(p.colors)) a.append(palStrip(p.colors));
  return a;
}
function row(app, pics, size = "", o = {}) {
  return h("div", { class: `ix-row ${size}` }, (pics || []).map((p) => tile(app, p, { ...o, size })), h("i"));
}
function palStrip(colors) { return h("span", { class: "pal", "aria-hidden": "true" }, colors.map(([c, f]) => h("span", { style: { flex: String(f), background: c } }))); }
function palette(colors, cls = "") { return h("div", { class: `ix-pal ${cls}` }, colors.map(([c, f]) => h("span", { style: { flex: String(f), background: c }, title: c }))); }
function empty(msg) { return h("div", { class: "ax-empty" }, msg); }
function seg(items, on, pick) {
  return h("div", { class: "ax-seg", role: "group" }, items.map(([k, t]) => h("button", { type: "button", class: k === on ? "on" : "", onclick: () => pick(k) }, t)));
}
function sec(title, right) { return h("div", { class: "ax-sec" }, h("h3", {}, title), right || null); }

// A photo of one's own, made smaller in the browser (CLIP sees 224 pixels)
// and sent as base64 JPEG.
async function shrink(file) {
  if (!file || !/^image\//.test(file.type || "image/")) throw new Error("that is not a picture");
  const url = URL.createObjectURL(file);
  try {
    const img = await new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = () => rej(new Error("this browser cannot read that picture")); i.src = url; });
    const s = Math.min(1, 1024 / Math.max(img.naturalWidth, img.naturalHeight));
    const c = document.createElement("canvas");
    c.width = Math.max(1, Math.round(img.naturalWidth * s));
    c.height = Math.max(1, Math.round(img.naturalHeight * s));
    c.getContext("2d").drawImage(img, 0, 0, c.width, c.height);
    return { data: c.toDataURL("image/jpeg", 0.9), name: file.name || "your photo", w: img.naturalWidth, h: img.naturalHeight };
  } finally { URL.revokeObjectURL(url); }
}
function fileOf(dt) {
  for (const it of dt?.items || []) if (it.kind === "file" && /^image\//.test(it.type)) return it.getAsFile();
  return [...(dt?.files || [])].find((f) => /^image\//.test(f.type)) || null;
}

// ---------------------------------------------------------------- the page

function render(el, ctx) {
  if (!document.getElementById("ix-css")) document.head.append(h("style", { id: "ix-css" }, CSS));
  const app = {
    ctx,
    queries: [],
    took: 0,
    photo: null,
    home: null,
    api: async (path, opts) => {
      const j = await ctx.api(path, opts);
      if (j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...parts),
    go: (...parts) => { location.hash = ctx.href(...parts); },
    // The address of this page with other parts, without a new step in history.
    put: (...parts) => { try { history.replaceState(history.state, "", ctx.href(...parts)); } catch {} },
  };
  const root = h("div", { class: "ax" });
  fill(el, root);
  const input = h("input", { type: "search", placeholder: "Describe a picture, or drop one here", autocomplete: "off", spellcheck: "false", "aria-label": "Search pictures" });
  const box = h("div", { class: "ax-box" }, input, h("span", { class: "kbd" }, "/"));
  box.insertAdjacentHTML("afterbegin", SEARCH);
  const count = h("small", {}, "free pictures");
  const brand = h("a", { class: "ax-brand", href: app.href("") });
  brand.innerHTML = ICON;
  brand.append(h("span", {}, "Image Explorer ", count));
  const photoBtn = h("a", { class: "ax-btn ix-pb", href: app.href("photo"), html: `${CAMERA}<span>Search with a picture</span>` });
  const nav = h("nav", { class: "ax-nav", "aria-label": "Sections" });
  const main = h("main", { class: "ax-main" });
  const sqlPanel = h("div", { class: "ax-sqlp", role: "dialog", "aria-label": "The SQL behind this page" });
  const sqlLink = h("a", { href: "#", onclick: (e) => { e.preventDefault(); sqlPanel.classList.toggle("on"); } }, "The SQL behind this page");
  root.append(h("header", { class: "ax-top" }, h("div", { class: "ax-bar" }, brand, box, photoBtn), nav), main, sqlPanel,
    h("footer", { class: "ax-foot" }, "Pictures from PD12M by Spawning, each public domain or CC0 and credited to the collection that holds it. · ",
      h("a", { href: app.href("built") }, "How it's built"), " · ", sqlLink, " · Built on ChloeDB"));

  function drawSql() {
    fill(sqlPanel, h("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "6px" } },
      h("b", {}, `The SQL behind this page: ${app.queries.length} ${app.queries.length === 1 ? "statement" : "statements"}, ${Math.round(app.took || 0)} ms in all`),
      h("button", { class: "ax-btn ghost", onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, q.what, h("span", {}, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`)),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", {}, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(q.params)}` : ""))));
  }
  drawSql();

  const PAGES = [["", "Search"], ["wander", "Wander"], ["colors", "Colors"], ["mix", "Mix"], ["bridge", "Bridge"], ["detail", "A detail"]];
  const UNDER = { "": ["search", "picture", "more"], wander: ["theme"], detail: ["photo"] };
  function drawNav(page) {
    fill(nav, PAGES.map(([p, t]) => h("a", { href: app.href(p), class: page === p || (UNDER[p] || []).includes(page) ? "on" : "" }, t)),
      h("span", { class: "sp" }), h("a", { href: app.href("built"), class: page === "built" ? "on" : "" }, "How it's built"));
  }

  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); const q = input.value.trim(); if (q) app.go("search", q); }
  });
  document.addEventListener("keydown", (e) => { if (e.key === "/" && document.activeElement !== input && !/INPUT|TEXTAREA|SELECT/.test(document.activeElement?.tagName || "")) { e.preventDefault(); input.focus(); input.select(); } });
  // A picture dropped or pasted on the box is a search with it.
  const take = async (file) => { if (!file) return; app.photo = file; if ((ctx.path() || [])[0] === "photo") show(["photo"]); else app.go("photo"); };
  box.addEventListener("dragover", (e) => { e.preventDefault(); box.classList.add("drop"); });
  box.addEventListener("dragleave", () => box.classList.remove("drop"));
  box.addEventListener("drop", (e) => { e.preventDefault(); box.classList.remove("drop"); take(fileOf(e.dataTransfer)); });
  input.addEventListener("paste", (e) => { const f = fileOf(e.clipboardData); if (f) { e.preventDefault(); take(f); } });

  let current = null, seq = 0;
  async function show(path) {
    path = path || ctx.path() || [];
    current = path;
    const me = ++seq;
    const alive = () => me === seq;
    const [page, ...rest] = path;
    drawNav(page || "");
    sqlPanel.classList.remove("on");
    scrollTo({ top: 0 });
    if (page !== "search") input.value = "";
    try {
      switch (page || "") {
        case "": return await homePage(app, main, alive);
        case "search": input.value = rest[0] || ""; return await searchPage(app, main, rest[0] || "", new URLSearchParams(rest[1] || ""), alive);
        case "picture": return await picturePage(app, main, rest[0], alive);
        case "more": return await morePage(app, main, rest[0], alive);
        case "photo": return await detailPage(app, main, null, null, alive, true);
        case "detail": return await detailPage(app, main, rest[0] || null, rest[1] || null, alive, false);
        case "mix": return await mixPage(app, main, rest[0] || null, rest[1] || "", rest[2], alive);
        case "bridge": return await bridgePage(app, main, rest[0] || null, rest[1] || null, rest[2], alive);
        case "wander": return await wanderPage(app, main, rest[0] || "", alive);
        case "theme": return await themePage(app, main, rest[0], rest[1] || "", rest[2] || "", alive);
        case "colors": return await colorsPage(app, main, rest[0] || "", rest[1] || "", alive);
        case "built": return await builtPage(app, main, alive);
        default: fill(main, empty("No such page."));
      }
    } catch (e) {
      if (alive()) fill(main, h("div", { class: "ax-err" }, String(e.message || e)));
    }
  }
  app.show = show;
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) show(p); });
  homeData(app).then((j) => { count.textContent = `${words(j.total)} free pictures`; input.placeholder = `Describe a picture, or drop one here (${short(j.total)} pictures)`; }).catch(() => {});
  show(ctx.path());
}

// The front page's data, asked once.
function homeData(app) {
  app.home ||= app.ctx.api("/v1/home").catch((e) => { app.home = null; throw e; });
  return app.home;
}

// ------------------------------------------------------------------ home

async function homePage(app, main, alive) {
  fill(main, h("section", { class: "ax-hero" }, h("h1", {}, "Search free pictures by what is in them"), h("p", {}, ...skel(2))), skelRow(14, "lg"));
  const j = await app.api("/v1/home");
  if (!alive()) return;
  const few = j.few || [];
  const pick = (i) => few[i % Math.max(1, few.length)];
  const museums = Math.max(0, (j.collections || 0) - 2);
  const ex = (j.examples || []).map((x) => h("a", { href: app.href("search", x.q) }, x.pic ? pimg(app, x.pic.id, "t", x.pic.title) : h("span", { class: "ph" }), h("span", {}, h("span", { class: "k" }, "Words"), h("span", { class: "q" }, x.q))));
  ex.push(h("a", { href: app.href("photo") }, h("span", { class: "ph ic", html: CAMERA }), h("span", {}, h("span", { class: "k" }, "Your photo"), h("span", { class: "q" }, "Drop in a photo and find it in paintings and prints"))));
  ex.push(h("a", { href: app.href("colors", "1f3a6e,e8792b") }, h("span", { class: "ph sw" }, h("i", { style: { background: "#1f3a6e" } }), h("i", { style: { background: "#e8792b" } })), h("span", {}, h("span", { class: "k" }, "Colors"), h("span", { class: "q" }, "Navy and orange"))));
  ex.push(h("a", { href: app.href("wander") }, pick(3) ? pimg(app, pick(3).id, "t", "") : h("span", { class: "ph" }), h("span", {}, h("span", { class: "k" }, "Wander"), h("span", { class: "q" }, "A map of the collection's themes"))));
  fill(main,
    h("section", { class: "ax-hero" },
      h("h1", {}, `Search ${words(j.total)} free pictures by what is in them`),
      h("p", {}, `Paintings, prints, photographs, maps, manuscripts and nature photos from Wikimedia Commons, iNaturalist and ${fmt(museums)} museums, archives and libraries. Every one is public domain or CC0, so you can use it for anything.`),
      h("div", { class: "ax-stats" }, h("span", {}, h("b", {}, short(j.total)), "pictures"), h("span", {}, h("b", {}, fmt(j.collections)), "collections"),
        h("span", {}, h("b", {}, short(j.commons)), "from Wikimedia Commons"), h("span", {}, h("b", {}, short(j.inaturalist)), "nature photos from iNaturalist"), h("span", {}, h("b", {}, short(j.museums)), "from museums and archives")),
      h("div", { class: "ix-ex" }, ex)),
    sec(`A few of the ${short(j.total)}`, h("a", { href: app.href("wander") }, "Wander all of them")),
    row(app, few.slice(0, 28), "lg"),
    h("div", { class: "ax-sec", style: { marginTop: "34px" } }, h("h3", {}, "Ways in that word search can't do")),
    h("div", { class: "ix-ways" }, WAYS.map((w, i) => h("a", { class: "ix-way", href: app.href(...w.href) },
      h("span", { class: "th" }, [0, 1, 2].map((k) => (pick(i * 3 + k + 7) ? pimg(app, pick(i * 3 + k + 7).id, "t", "") : null))),
      h("span", { class: "b" }, h("h3", {}, w.t), h("p", {}, w.d))))));
}

const WAYS = [
  { t: "Search with your photo", d: "Drop in a photo of your own and see the same thing in nature photos, paintings and old prints.", href: ["photo"] },
  { t: "Search a detail", d: "Draw a box on any picture and find pictures of what is inside it.", href: ["detail"] },
  { t: "A picture plus words", d: "Start from a picture and move it toward some words: this river scene, in winter.", href: ["mix"] },
  { t: "Bridge two pictures", d: "The pictures on the way from one to another, a step at a time.", href: ["bridge"] },
  { t: "Colors", d: "Pick up to three colors and get the pictures that hold them.", href: ["colors"] },
  { t: "Wander", d: "Every picture sorted into themes by what it looks like, laid out as a map.", href: ["wander"] },
  { t: "Steer with thumbs", d: "Thumbs up the pictures you want more of and hide the rest; the search moves.", href: ["search", "a lighthouse in a storm"] },
  { t: "The same subject in another medium", d: "On any picture: its subject as a pencil drawing, an oil painting or an old photograph.", href: ["search", "a red fox"] },
];

// ---------------------------------------------------------------- search

const FACETS = [["kind", "Kind", KIND_WORDS], ["licence", "Licence", LICENCE], ["source", "Collection", null], ["shape", "Shape", SHAPES], ["size", "Size", SIZES]];

async function searchPage(app, main, q, opts, alive) {
  if (!q.trim()) { app.go(""); return; }
  const list = (k) => (opts.get(k) || "").split(",").filter(Boolean);
  const like = list("like"), hide = list("hide");
  const put = (o, k, v) => { if (v == null || v === "" || (Array.isArray(v) && !v.length)) o.delete(k); else o.set(k, Array.isArray(v) ? v.join(",") : v); return o; };
  const set = (k, v) => put(new URLSearchParams(opts), k, v);
  const go = (o) => app.go("search", q, o.toString());
  const left = h("aside", { class: "ax-left", "aria-label": "Filters" }, skel(8));
  const res = h("section", { "aria-label": "Results" }, skelRow(30));
  fill(main, h("div", { class: "ax-grid two-l" }, left, res));
  const params = new URLSearchParams({ q });
  for (const k of ["like", "hide", "steer", "order", "kind", "source", "licence", "shape", "size"]) if (opts.get(k)) params.set(k, opts.get(k));
  const j = await app.api(`/v1/search?${params}`, { timeout: 60000 });
  if (!alive()) return;

  // Facets over the closest 2,000.
  fill(left, FACETS.map(([k, title, names]) => {
    const rows = j.facets?.[k] || [];
    if (!rows.length) return null;
    const on = new Set(list(k));
    return h("div", { class: "ax-facet" }, h("h4", {}, title), rows.map((r) => h("label", { class: "r" },
      h("input", { type: "checkbox", checked: on.has(r.v), onchange: (e) => { const s = new Set(list(k)); e.target.checked ? s.add(r.v) : s.delete(r.v); go(set(k, [...s])); } }),
      h("span", { class: "l" }, names?.[r.v] || r.v), h("span", { class: "n" }, fmt(r.n)))));
  }), h("p", { class: "ix-note" }, "Counts are over the 2,000 closest pictures."));

  const filtered = ["kind", "source", "licence", "shape", "size"].filter((k) => opts.get(k));
  const variety = opts.get("order") === "variety";
  const grid = h("div", { class: "ix-row" });
  let page = 0;
  const vote = (p) => {
    const d = h("div", { style: { flex: flex(p) }, class: like.includes(p.id) ? "liked" : "" });
    const up = h("button", { type: "button", class: `up${like.includes(p.id) ? " on" : ""}`, "aria-label": "More like this one", "aria-pressed": String(like.includes(p.id)), html: UP, title: "More like this one",
      onclick: () => { const l = like.includes(p.id) ? like.filter((x) => x !== p.id) : [...like, p.id].slice(-12); go(set("like", l)); } });
    const down = h("button", { type: "button", class: "down", "aria-label": "Hide this and pictures like it", html: HIDE, title: "Hide this and pictures like it",
      onclick: () => go(put(set("like", like.filter((x) => x !== p.id)), "hide", [...hide, p.id].slice(-12))) });
    d.append(h("a", { href: app.href("picture", p.id), title: p.title }, pimg(app, p.id, "t", p.title)), h("span", { class: "vote" }, up, down), cap(p));
    return d;
  };
  const append = (pics) => { grid.querySelector(":scope > i")?.remove(); grid.append(...pics.map(vote), h("i")); };
  append(j.pictures || []);
  const moreBtn = h("button", { type: "button", class: "ax-btn", onclick: async () => {
    moreBtn.disabled = true; moreBtn.textContent = "Finding more…";
    try {
      const m = await app.api(`/v1/search?${params}&page=${++page}`, { timeout: 60000 });
      append(m.pictures || []);
      moreBtn.style.display = m.more && page < 40 ? "" : "none";
    } catch (e) { moreBtn.textContent = String(e.message || e); return; }
    moreBtn.disabled = false; moreBtn.textContent = "More pictures";
  } }, "More pictures");
  moreBtn.style.display = j.more ? "" : "none";

  // The pictures liked and hidden, and how strongly they steer.
  const thumb = (id) => h("a", { href: app.href("picture", id), title: "Open it" }, pimg(app, id, "t", ""));
  const steer = like.length || hide.length ? h("div", { class: "ix-steer" },
    like.length ? h("span", { class: "g up" }, h("b", {}, "Liked"), like.map(thumb)) : null,
    hide.length ? h("span", { class: "g dn" }, h("b", {}, "Hidden"), hide.map(thumb)) : null,
    h("span", { class: "g" }, h("b", {}, "Steer"), seg([["", "Gently"], ["strong", "Strongly"]], opts.get("steer") || "", (k) => go(set("steer", k)))),
    h("button", { type: "button", class: "ax-btn ghost", onclick: () => { const o = set("like", null); o.delete("hide"); o.delete("steer"); go(o); } }, "Reset"),
    h("span", { class: "why" }, `The results lean toward the ${like.length ? `${like.length === 1 ? "picture" : `${like.length} pictures`} you liked` : ""}${like.length && hide.length ? " and away from " : hide.length ? "away from " : ""}${hide.length ? `the ${hide.length === 1 ? "one" : hide.length} you hid` : ""}: their vectors are added to the words' and taken away.`)) : null;

  fill(res,
    h("div", { class: "ax-head" }, h("span", { class: "t" }, h("b", {}, q), ` · the closest of ${short(j.compared)} pictures by what is in them${filtered.length ? `, ${filtered.map((k) => list(k).map((v) => FACETS.find((f) => f[0] === k)[2]?.[v] || v).join(" or ")).join(", ")}` : ""}`),
      filtered.length ? h("a", { class: "ax-chip f", href: app.href("search", q, (() => { const o = new URLSearchParams(opts); filtered.forEach((k) => o.delete(k)); return o.toString(); })()) }, "Clear filters ×") : null,
      h("span", { class: "sp" }), h("button", { type: "button", class: "ax-btn ix-fbtn", onclick: () => left.classList.toggle("open") }, filtered.length ? `Filters · ${filtered.length}` : "Filters"),
      seg([["", "Best match"], ["variety", "More variety"]], variety ? "variety" : "", (k) => go(set("order", k)))),
    steer || h("p", { class: "ix-note", style: { margin: "0 0 10px" } }, "Thumbs up a picture to see more like it; hide one to see fewer like it."),
    (j.pictures || []).length ? grid : empty(filtered.length ? "No pictures pass these filters among the closest. Clear a filter." : "Nothing found."),
    h("div", { style: { display: "flex", justifyContent: "center", marginTop: "18px" } }, moreBtn));
}

// --------------------------------------------------------------- picture

const CREDIT_WORDS = {
  CC0: (src) => `${src} gave up all rights to this picture (CC0), so it is free for any use, including commercial, with no permission needed. A credit is kind, not required.`,
  PD: (src) => `${src} marks this picture as public domain: no one holds rights to it, so it is free for any use. A credit is kind, not required.`,
  "": (src) => `PD12M lists this picture from ${src} as free to use, but its record names no licence: check at the collection before using it.`,
};

async function picturePage(app, main, id, alive) {
  fill(main, h("div", { class: "ix-two", style: { marginTop: "10px" } }, h("div", { class: "ix-wait", style: { height: "520px", borderRadius: "10px" } }), h("div", {}, skel(9))));
  const j = await app.api(`/v1/picture/${enc(id)}`, { timeout: 60000 });
  if (!alive()) return;
  const p = j.picture;
  const src = p.source || "the collection";
  const credit = `${p.title}. ${src}, ${p.licence === "CC0" ? "CC0 1.0" : p.licence === "PD" ? "public domain" : "licence not given"}. Found with Image Explorer.`;
  const pal = h("dd", {}, Array.isArray(p.colors) ? palette(p.colors) : h("span", { class: "dim" }, "made with the picture's copy, in a moment"));
  const opener = h("span", {});
  const drawLink = (link) => fill(opener, link?.page ? h("a", { class: "ax-btn ghost", href: link.page, target: "_blank", rel: "noopener" }, `Open at ${src}`) : null);
  drawLink(p.link);
  // Its colors and its page at Commons are found with the picture's copy:
  // asked again once the copy is shown.
  const known = () => app.ctx.api(`/v1/known/${enc(id)}`).then((k) => { if (Array.isArray(k.colors)) fill(pal, palette(k.colors)); if (k.link) drawLink(k.link); }).catch(() => {});
  const big = pimg(app, p.id, "l", p.caption || p.title, () => { if (!Array.isArray(p.colors) || !p.link) setTimeout(known, p.source === "Wikimedia Commons" ? 1500 : 200); });
  const copy = h("button", { type: "button", class: "ax-btn", onclick: async () => { try { await navigator.clipboard.writeText(credit); copy.textContent = "Copied ✓"; } catch { copy.textContent = "Select the line below"; } } }, "Copy credit");
  const same = j.same || [];
  fill(main,
    h("div", { class: "ix-two", style: { marginTop: "10px" } },
      h("div", {},
        h("div", { class: "ix-pic ix-big" }, big),
        h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap", marginTop: "10px" } },
          h("a", { class: "ax-btn", href: app.href("detail", p.id), html: `${CORNERS}<span>Search a detail of it</span>` }),
          h("a", { class: "ax-btn", href: app.href("mix", p.id), html: `${PLUS}<span>Add words to it</span>` }),
          h("a", { class: "ax-btn", href: app.href("bridge", p.id), html: `${ARROWS}<span>Bridge to another picture</span>` }))),
      h("div", {},
        h("div", { class: "ax-ph" },
          h("div", { class: "ey" }, h("span", { class: `ix-tag${p.licence ? " pd" : ""}` }, p.licence ? `Free to use · ${p.licence}` : "Licence not given"), h("span", {}, KIND_WORDS[p.kind] || p.kind), h("span", {}, "·"), h("span", {}, src)),
          h("h1", {}, p.title),
          h("div", { class: "au", style: { fontSize: "13px", color: "var(--muted)" } }, `Title made from the description below; the collection's own title and date are on its page there.`),
          p.caption ? h("p", { class: "abs", style: { fontSize: "14px", color: "var(--text-2)" } }, h("b", { style: { color: "var(--text)", fontWeight: "600" } }, "Described by a model, so it can be wrong: "), `“${p.caption}”`) : null,
          h("div", { class: "acts" }, p.file ? h("a", { class: "ax-btn pri", href: p.file, target: "_blank", rel: "noopener", download: "" }, "Download full size") : null, copy, opener)),
        h("div", { class: "ax-card" }, h("h4", {}, "Use it"),
          h("div", { class: "ix-credit" }, (CREDIT_WORDS[p.licence] || CREDIT_WORDS[""])(src), h("div", { class: "line" }, credit)),
          h("dl", { class: "ix-kv", style: { margin: "10px 0 0" } },
            h("dt", {}, "Kind"), h("dd", {}, `${KIND_WORDS[p.kind] || p.kind} (guessed from the picture)`),
            h("dt", {}, "Collection"), h("dd", {}, src),
            h("dt", {}, "Licence"), h("dd", {}, p.licence === "CC0" ? "CC0 1.0, no rights reserved" : p.licence === "PD" ? "Public domain mark 1.0" : "not given"),
            h("dt", {}, "Size"), h("dd", {}, `${fmt(p.w)} × ${fmt(p.h)} px, ${(p.mp || 0).toFixed(1)} MP`),
            j.theme ? [h("dt", {}, "Theme"), h("dd", {}, h("a", { href: app.href("theme", j.theme.id), style: { color: "var(--accent)" } }, j.theme.name), ` · ${fmt(j.theme.n)} pictures`)] : null,
            h("dt", {}, "Colors"), pal)),
        same.length ? h("div", { class: "ax-card" }, h("h4", {}, "The same picture elsewhere"),
          same.map((s) => h("a", { class: "ix-dup", href: app.href("picture", s.id) }, pimg(app, s.id, "t", ""), h("div", {}, h("div", {}, s.source), h("div", { class: "m" }, `${fmt(s.w)} × ${fmt(s.h)} px · ${s.licence === "CC0" ? "CC0" : s.licence === "PD" ? "public domain" : "no licence given"}${s.w * s.h > p.w * p.h ? ` · this copy is larger` : ""}`)))),
          h("p", { class: "ix-note" }, "Found because the two pictures are nearly the same, not because their records say so.")) : null)),
    sec("More like this", h("a", { href: app.href("more", p.id) }, "See more")),
    row(app, j.more, "lg"),
    (j.other || []).some((g) => g.pictures?.length) ? [
      h("div", { class: "ax-sec", style: { marginTop: "30px" } }, h("h3", {}, "The same subject in another medium"), h("span", { class: "note" }, "This picture plus a few words, like “a pencil drawing of”, among the pictures of that kind")),
      h("div", { class: "ix-styles" }, j.other.filter((g) => g.pictures?.length).map((g) => h("div", {}, h("h4", {}, g.label), row(app, g.pictures.slice(0, 6), "sm"))))] : null);
}

async function morePage(app, main, id, alive) {
  fill(main, h("div", { class: "ax-ph" }, skel(2)), skelRow(30));
  const j = await app.api(`/v1/more/${enc(id)}`, { timeout: 60000 });
  if (!alive()) return;
  const grid = row(app, j.pictures);
  let page = 0;
  const btn = h("button", { type: "button", class: "ax-btn", onclick: async () => {
    btn.disabled = true;
    try {
      const m = await app.api(`/v1/more/${enc(id)}?page=${++page}`, { timeout: 60000 });
      grid.querySelector(":scope > i")?.remove();
      grid.append(...(m.pictures || []).map((p) => tile(app, p)), h("i"));
      btn.style.display = m.more ? "" : "none";
    } finally { btn.disabled = false; }
  } }, "More pictures");
  btn.style.display = j.more ? "" : "none";
  fill(main,
    h("div", { class: "ax-head", style: { margin: "10px 0 12px", alignItems: "center" } },
      h("a", { href: app.href("picture", id), class: "ix-of" }, pimg(app, id, "t", "")),
      h("div", {}, h("div", { class: "ix-eyebrow" }, "More like"), h("h1", { class: "ix-h1" }, h("a", { href: app.href("picture", id) }, j.of?.title || "this picture")))),
    grid, h("div", { style: { display: "flex", justifyContent: "center", marginTop: "18px" } }, btn));
}

// ---------------------------------------------------- a detail, your photo

async function detailPage(app, main, id, boxArg, alive, photoFirst) {
  // With no picture named, a painting with a clear subject to draw on.
  if (!id) {
    const home = await homeData(app).catch(() => null);
    if (!alive()) return;
    const few = home?.few || [];
    id = (home?.detail || few.find((p) => p.kind === "Painting") || few[0] || {}).id;
  }
  const detail = id ? detailSection(app, id, boxArg, alive) : h("section", {}, empty("No picture to draw on yet."));
  const photo = photoSection(app, alive);
  const rule = h("div", { style: { borderTop: "1px solid var(--line)", margin: "34px 0 22px" } });
  fill(main, photoFirst ? [photo, rule, detail] : [detail, rule, photo]);
}

function detailSection(app, id, boxArg, alive) {
  let b = (boxArg || "").split(",").map(Number);
  if (b.length !== 4 || b.some((x) => !(x >= 0 && x <= 1))) b = [0.3, 0.3, 0.7, 0.7];
  let kind = "";
  const sel = h("div", { class: "ix-box" }, ["nw", "ne", "sw", "se"].map((c) => h("span", { class: `h ${c}`, "data-c": c })));
  const picWrap = h("div", { class: "ix-pic ix-draw keep" });
  const out = h("div", {}, skelRow(12));
  const lead = h("p", { class: "lead" }, "Draw a box on the picture and search for what is inside it.");
  const of = h("p", { class: "ix-note" });
  const place = () => Object.assign(sel.style, { left: `${b[0] * 100}%`, top: `${b[1] * 100}%`, width: `${(b[2] - b[0]) * 100}%`, height: `${(b[3] - b[1]) * 100}%` });
  place();
  let n = 0;
  const run = async () => {
    const me = ++n;
    fill(out, skelRow(12));
    app.put("detail", id, b.map((x) => x.toFixed(3)).join(","));
    try {
      const j = await app.api("/v1/detail", { body: { id, box: b, kind }, timeout: 90000 });
      if (me !== n || !alive()) return;
      fill(of, `${j.of?.source || ""}${j.of?.licence ? ` · ${j.of.licence}` : ""}. Drag the box, or draw a new one.`);
      if (kind) fill(out, h("h4", {}, "Pictures of what is in the box"), row(app, j.pictures));
      else fill(out,
        h("h4", {}, "Pictures of what is in the box"), row(app, (j.pictures || []).slice(0, 12)),
        (j.photos || []).length ? [h("h4", {}, "Photos of it"), row(app, j.photos.slice(0, 8), "sm")] : null,
        (j.art || []).length ? [h("h4", {}, "In paintings, prints and drawings"), row(app, j.art.slice(0, 8), "sm")] : null);
    } catch (e) { if (me === n) fill(out, h("div", { class: "ax-err" }, String(e.message || e))); }
  };
  // Draw, move or stretch the box; a search when the pointer is let go.
  let drag = null;
  picWrap.addEventListener("pointerdown", (e) => {
    const r = picWrap.getBoundingClientRect();
    const x = (e.clientX - r.left) / r.width, y = (e.clientY - r.top) / r.height;
    const c = e.target.dataset?.c;
    if (c) drag = { mode: "corner", c };
    else if (e.target === sel) drag = { mode: "move", x, y, b0: [...b] };
    else { drag = { mode: "draw", x, y }; b = [x, y, x, y]; place(); }
    picWrap.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  picWrap.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const r = picWrap.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), y = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
    if (drag.mode === "draw") b = [Math.min(drag.x, x), Math.min(drag.y, y), Math.max(drag.x, x), Math.max(drag.y, y)];
    else if (drag.mode === "move") {
      const w = drag.b0[2] - drag.b0[0], hh = drag.b0[3] - drag.b0[1];
      const l = Math.min(1 - w, Math.max(0, drag.b0[0] + x - drag.x)), t = Math.min(1 - hh, Math.max(0, drag.b0[1] + y - drag.y));
      b = [l, t, l + w, t + hh];
    } else {
      if (drag.c.includes("w")) b[0] = Math.min(x, b[2] - 0.03); else b[2] = Math.max(x, b[0] + 0.03);
      if (drag.c.includes("n")) b[1] = Math.min(y, b[3] - 0.03); else b[3] = Math.max(y, b[1] + 0.03);
    }
    place();
  });
  const end = () => {
    if (!drag) return;
    drag = null;
    if (b[2] - b[0] < 0.04 || b[3] - b[1] < 0.04) { b = [Math.max(0, b[0] - 0.1), Math.max(0, b[1] - 0.1), Math.min(1, b[2] + 0.1), Math.min(1, b[3] + 0.1)]; place(); }
    run();
  };
  picWrap.addEventListener("pointerup", end);
  picWrap.addEventListener("pointercancel", end);
  const kinds = h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap", margin: "0 0 4px" } });
  const drawKinds = () => fill(kinds, [["", "Any kind"], ["Painting,Drawing,Print,Illustration", "Only art"], ["Photo,Nature,Old photo", "Only photos"]].map(([k, t]) => h("button", { type: "button", class: `ax-btn${kind === k ? " on" : ""}`, onclick: () => { kind = k; drawKinds(); run(); } }, t)),
    h("a", { class: "ax-btn ghost", href: app.href("picture", id) }, "Back to the whole picture"));
  drawKinds();
  app.ctx.api(`/v1/picture/${enc(id)}?brief=1`).then((j) => {
    if (!alive()) return;
    const p = j.picture;
    if (p?.w && p?.h) { picWrap.style.aspectRatio = `${p.w} / ${p.h}`; picWrap.style.width = `min(100%, calc(72vh * ${(p.w / p.h).toFixed(4)}))`; }
    fill(lead, `Draw a box on any picture and search for what is inside it. This is ${p ? `“${p.title}” from ${p.source}` : "a picture from the collection"}; open any other picture and press “Search a detail of it” to draw on that one.`);
  }).catch(() => {});
  picWrap.append(pimg(app, id, "l", "The picture to draw a box on", () => run()), sel);
  return h("section", { class: "ix-split", "aria-label": "Search a detail" },
    h("div", {}, picWrap, of),
    h("div", {}, h("h2", {}, "Search a detail"), lead, kinds, out));
}

function photoSection(app, alive) {
  const out = h("div", {});
  const wordsIn = h("input", { type: "text", class: "ix-in", placeholder: "Add words, like “in winter” (optional)", "aria-label": "Words to add to the photo" });
  const fileIn = h("input", { type: "file", accept: "image/*", style: { display: "none" } });
  const camIn = h("input", { type: "file", accept: "image/*", capture: "environment", style: { display: "none" } });
  const drop = h("div", { class: "ix-drop", tabindex: "0" }, h("div", { html: CAMERA, style: { color: "var(--accent)" } }), h("div", {}, h("b", {}, "Drop a photo here"), ", paste one, or ", h("a", { href: "#", onclick: (e) => { e.preventDefault(); fileIn.click(); } }, "choose a file")),
    h("div", { class: "ix-note" }, "Your photo is used for this search and then thrown away. It is never added to the collection."));
  let last = null;
  const run = async (file) => {
    let ph;
    try { ph = file ? await shrink(file) : last; } catch (e) { fill(out, h("div", { class: "ax-err" }, String(e.message || e))); return; }
    if (!ph) return;
    last = ph;
    fill(drop, h("img", { src: ph.data, alt: "Your photo", style: { width: "100%", display: "block", borderRadius: "8px" } }),
      h("div", { style: { marginTop: "10px" } }, h("b", {}, ph.name), ` · ${fmt(ph.w)} × ${fmt(ph.h)} px`),
      h("div", { class: "ix-note" }, "Used for this search and then thrown away. It is never added to the collection."));
    fill(out, skelRow(12));
    try {
      const j = await app.api("/v1/photo", { body: { data: ph.data, words: wordsIn.value.trim() }, timeout: 90000 });
      if (!alive()) return;
      fill(out,
        h("h4", {}, "The closest pictures"), row(app, (j.pictures || []).slice(0, 12)),
        (j.photos || []).length ? [h("h4", {}, "Photos like it"), row(app, j.photos.slice(0, 8), "sm")] : null,
        (j.art || []).length ? [h("h4", {}, "In paintings, prints and old photos"), row(app, j.art.slice(0, 8), "sm")] : null);
    } catch (e) { fill(out, h("div", { class: "ax-err" }, String(e.message || e))); }
  };
  fileIn.addEventListener("change", () => run(fileIn.files[0]));
  camIn.addEventListener("change", () => run(camIn.files[0]));
  drop.addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("on"); });
  drop.addEventListener("dragleave", () => drop.classList.remove("on"));
  drop.addEventListener("drop", (e) => { e.preventDefault(); drop.classList.remove("on"); run(fileOf(e.dataTransfer)); });
  const paste = (e) => { if (!alive()) { removeEventListener("paste", paste); return; } const f = fileOf(e.clipboardData); if (f) { e.preventDefault(); run(f); } };
  addEventListener("paste", paste);
  wordsIn.addEventListener("keydown", (e) => { if (e.key === "Enter" && last) run(null); });
  if (app.photo) { const f = app.photo; app.photo = null; setTimeout(() => run(f), 0); }
  return h("section", { class: "ix-split", "aria-label": "Search with your photo" },
    h("div", {}, drop, fileIn, camIn,
      h("div", { style: { display: "flex", gap: "8px", flexWrap: "wrap", marginTop: "10px" } },
        h("button", { type: "button", class: "ax-btn", onclick: () => fileIn.click() }, "Choose a photo"),
        h("button", { type: "button", class: "ax-btn ghost", onclick: () => camIn.click() }, "Take a photo")),
      h("div", { style: { marginTop: "10px" } }, wordsIn)),
    h("div", {}, h("h2", {}, "Search with your photo"),
      h("p", { class: "lead" }, "Drop in a photo of your own and see where it shows up among the free pictures: photos of the same thing, and how painters, printmakers and photographers a century ago saw it."),
      out));
}

// ------------------------------------------------------------ mix, bridge

async function mixPage(app, main, id, q, wArg, alive) {
  const head = h("div", { class: "ax-ph", style: { marginTop: "10px" } }, h("h1", { style: { fontSize: "24px" } }, "A picture plus words"),
    h("p", { class: "ix-note", style: { fontSize: "14px", color: "var(--text-2)", margin: "0 0 12px" } }, "Start from a picture and add words to it. The search keeps what the picture looks like and moves it toward the words."));
  if (!id) {
    fill(main, head, skelRow(24));
    const home = await homeData(app).catch(() => null);
    if (!alive()) return;
    const few = home?.few || [];
    const ex = home?.mix;
    fill(main, head,
      ex?.pic ? h("a", { class: "ix-try", href: app.href("mix", ex.pic.id, ex.words) }, pimg(app, ex.pic.id, "t", ex.pic.title), h("span", { class: "op" }, "+"), h("span", { class: "w" }, `“${ex.words}”`), h("span", { class: "go" }, "Try it →")) : null,
      sec("Or pick a picture to start from", h("span", { class: "note" }, "or open any picture and press “Add words to it”")),
      h("div", { class: "ix-row" }, few.slice(0, 24).map((p, i) => tile(app, p, { href: app.href("mix", p.id, MIX_WORDS[i % MIX_WORDS.length]) })), h("i")));
    return;
  }
  let w = Math.min(1, Math.max(0, Number(wArg ?? 0.6) || 0));
  if (wArg === undefined) w = 0.6;
  const wordsIn = h("input", { type: "text", value: q, placeholder: "in winter, snow and ice", "aria-label": "Words to add", class: "ix-bare" });
  const first = h("div", { class: "ix-first" }, h("div", { class: "ix-wait" }));
  const slider = h("input", { type: "range", min: "0", max: "1", step: "0.05", value: String(w), "aria-label": "How much of the words", class: "ix-range" });
  const lot = h("div", {}, q ? skelRow(12) : empty("Add some words to the picture."));
  const other = h("div", {});
  fill(main, head,
    h("div", { class: "ix-mix" }, h("a", { href: app.href("picture", id) }, pimg(app, id, "t", "The picture to start from")), h("span", { class: "op", "aria-hidden": "true" }, "+"),
      h("label", { class: "w" }, h("small", {}, "Words"), wordsIn), h("span", { class: "op", "aria-hidden": "true" }, "="), first),
    h("div", { class: "ix-slider" }, h("span", {}, "How much of the words"), h("span", {}, "a little"), slider, h("span", {}, "a lot")),
    lot, other,
    h("div", { class: "ix-chips" }, h("span", { class: "ix-note" }, "Try: "), MIX_WORDS.map((m) => h("a", { class: "ax-chip", href: app.href("mix", id, m, String(w)) }, m))));
  let n = 0;
  const run = async () => {
    const qq = wordsIn.value.trim();
    if (!qq) return;
    const me = ++n;
    app.put("mix", id, qq, String(w));
    fill(lot, skelRow(12));
    const ow = w >= 0.5 ? 0.1 : 0.95;
    try {
      const [a, b] = await Promise.all([app.api(`/v1/mix?id=${enc(id)}&q=${enc(qq)}&w=${w}`), app.ctx.api(`/v1/mix?id=${enc(id)}&q=${enc(qq)}&w=${ow}`).catch(() => null)]);
      if (me !== n || !alive()) return;
      const pics = a.pictures || [];
      fill(first, pics[0] ? h("a", { href: app.href("picture", pics[0].id), title: pics[0].title }, pimg(app, pics[0].id, "t", pics[0].title)) : h("div", { class: "ix-wait" }));
      fill(lot, sec(w >= 0.5 ? "With a lot of the words" : "With a little of the words", h("span", { class: "note" }, `the picture's vector plus ${a.weight.toFixed(2)} × the words'`)), row(app, pics.slice(0, 18)));
      fill(other, b?.pictures?.length ? [sec(ow > 0.5 ? "With a lot of the words" : "With a little of the words"), row(app, b.pictures.slice(0, 10), "sm")] : null);
    } catch (e) { if (me === n) fill(lot, h("div", { class: "ax-err" }, String(e.message || e))); }
  };
  let t;
  wordsIn.addEventListener("keydown", (e) => { if (e.key === "Enter") run(); });
  slider.addEventListener("input", () => { w = Number(slider.value); clearTimeout(t); t = setTimeout(run, 250); });
  if (q) run(); else wordsIn.focus();
}
const MIX_WORDS = ["in winter, snow and ice", "at night", "as a watercolor", "in Japan", "in autumn colors", "under water", "a black and white photograph", "with a cat"];

async function bridgePage(app, main, a, b, stepsArg, alive) {
  const steps = [5, 7, 11].includes(Number(stepsArg)) ? Number(stepsArg) : 7;
  const head = (sub) => h("div", { class: "ax-head", style: { marginTop: "10px" } }, h("div", {}, h("h1", { class: "ix-h1" }, "Bridge two pictures"), h("p", { class: "ix-sub" }, sub)));
  if (!a || !b) {
    const which = a ? "Pick where to end" : "Pick where to start";
    const hrefOf = (p) => (a ? app.href("bridge", a, p.id) : app.href("bridge", p.id));
    const found = h("div", {});
    const q = h("input", { type: "search", class: "ix-in", placeholder: a ? "Search for the picture to end at" : "Search for the picture to start from", "aria-label": "Search for a picture" });
    q.addEventListener("keydown", async (e) => {
      if (e.key !== "Enter" || !q.value.trim()) return;
      fill(found, skelRow(12));
      try { const j = await app.api(`/v1/search?q=${enc(q.value.trim())}`); fill(found, h("div", { class: "ix-row" }, (j.pictures || []).slice(0, 24).map((p) => tile(app, p, { href: hrefOf(p) })), h("i"))); } catch (err) { fill(found, h("div", { class: "ax-err" }, String(err.message || err))); }
    });
    const exb = !a ? (await homeData(app).catch(() => null))?.bridge : null;
    if (!alive()) return;
    fill(main, head("Each step is the closest picture to a point on the way between the two."),
      exb?.a && exb?.b ? h("a", { class: "ix-try", href: app.href("bridge", exb.a.id, exb.b.id) }, pimg(app, exb.a.id, "t", exb.a.title), h("span", { class: "op" }, "→"), pimg(app, exb.b.id, "t", exb.b.title), h("span", { class: "w" }, `From ${exb.a.title.toLowerCase().replace(/^an? /, "a ")} to ${exb.b.title.toLowerCase().replace(/^an? /, "a ")}`), h("span", { class: "go" }, "Try it →")) : null,
      a ? h("div", { class: "ix-chosen" }, h("span", { class: "ix-note" }, "Starting from"), h("a", { href: app.href("picture", a) }, pimg(app, a, "t", ""))) : null,
      h("div", { style: { maxWidth: "560px", margin: "6px 0 4px" } }, q), found, skelRow(18));
    const home = await homeData(app).catch(() => null);
    if (!alive()) return;
    const few = (home?.few || []).filter((p) => p.id !== a);
    main.lastChild.replaceWith(h("div", {}, sec(which, h("span", { class: "note" }, "or search for one above")), h("div", { class: "ix-row" }, (a ? few.slice().reverse() : few).slice(0, 24).map((p) => tile(app, p, { href: hrefOf(p) })), h("i"))));
    return;
  }
  const path = h("div", { class: "ix-path" }, Array.from({ length: steps + 2 }, () => h("div", { class: "st" }, h("div", { class: "ix-wait", style: { height: "132px" } }))));
  fill(main, head("Each step is the closest picture to a point on the way between the two, each picture used once."), path);
  const j = await app.api(`/v1/bridge?a=${enc(a)}&b=${enc(b)}&steps=${steps}`, { timeout: 60000 });
  if (!alive()) return;
  const all = [j.a, ...(j.path || []), j.b];
  const stop = (p, i) => h("a", { class: `st${i === 0 || i === all.length - 1 ? " end keep" : ""}`, href: app.href("picture", p.id), title: p.title }, pimg(app, p.id, "t", p.title), h("div", { class: "l" }, i === 0 ? "Start" : i === all.length - 1 ? "End" : p.title));
  fill(main,
    h("div", { class: "ax-head", style: { marginTop: "10px" } },
      h("div", {}, h("h1", { class: "ix-h1" }, "Bridge two pictures"), h("p", { class: "ix-sub" }, `From “${j.a.title}” (${j.a.source}) to “${j.b.title}” (${j.b.source}). Each step is the closest picture to a point on the way between the two.`)),
      h("span", { class: "sp" }),
      seg([["5", "5 steps"], ["7", "7 steps"], ["11", "11 steps"]], String(steps), (k) => app.go("bridge", a, b, k)),
      h("a", { class: "ax-btn", href: app.href("bridge", b, a, String(steps)) }, "Swap ends")),
    h("div", { class: "ix-path" }, all.map((p, i) => [i ? h("span", { class: "ar", "aria-hidden": "true" }, "→") : null, stop(p, i)])),
    h("div", { style: { display: "flex", gap: "8px", marginTop: "14px", flexWrap: "wrap" } },
      h("a", { class: "ax-btn", href: app.href("bridge", b) }, "Keep the end, pick another start"),
      h("a", { class: "ax-btn", href: app.href("bridge", a) }, "Keep the start, pick another end")),
    sec("The steps, larger"), row(app, j.path || [], "lg"));
}

// ---------------------------------------------------------------- wander

async function wanderPage(app, main, group, alive) {
  const head = h("div", { class: "ax-head", style: { margin: "10px 0 12px" } });
  const map = h("div", { class: "ix-wmap" }, skel(6));
  fill(main, head, map);
  const j = await app.api("/v1/themes");
  if (!alive()) return;
  const ts = j.themes || [];
  const cols = Math.max(1, ...ts.map((t) => t.col + 1));
  // At first the map fits the page; + makes the islands larger (and the map scrolls).
  let size = 0;
  const zoom = (d) => { size = d > 0 ? Math.min(280, Math.max(160, size + 40)) : size <= 160 ? 0 : size - 40; map.style.setProperty("--isl", `${size}px`); };
  map.style.gridTemplateColumns = `repeat(${cols}, minmax(max(var(--isl), var(--isl-min, 0px)), 1fr))`;
  map.style.setProperty("--isl", "0px");
  const draw = (g) => {
    fill(head, h("div", {}, h("h1", { class: "ix-h1" }, "Wander"),
      h("p", { class: "ix-sub", style: { maxWidth: "820px" } }, `The ${short(j.total)} pictures sorted into ${ts.length} themes by what they look like, with themes that look alike next to each other. Each island shows the nine pictures closest to its middle. Open one to walk through it.`)),
      h("span", { class: "sp" }), seg([["", "Everything"], ["art", "Art"], ["photos", "Photos"], ["nature", "Nature"]], g, (k) => { app.put("wander", k); draw(k); }),
      h("span", { class: "ix-zoom" }, h("button", { type: "button", class: "ax-btn", "aria-label": "Smaller islands", onclick: () => zoom(-1) }, "−"),
        h("button", { type: "button", class: "ax-btn", "aria-label": "Larger islands", onclick: () => zoom(1) }, "+")));
    for (const a of map.querySelectorAll(".ix-isl")) a.classList.toggle("dim", !!g && a.dataset.g !== g);
  };
  fill(map, ts.map((t) => h("a", { class: "ix-isl", "data-g": t.group, href: app.href("theme", t.id), style: { gridRow: String(t.row + 1), gridColumn: String(t.col + 1) }, title: `${t.name}: ${fmt(t.n)} pictures` },
    (() => { const reps = t.reps || []; const k = reps.length >= 9 ? 3 : reps.length >= 4 ? 2 : 1; return h("span", { class: "tiles", style: { gridTemplateColumns: `repeat(${k}, minmax(0, 1fr))` } }, reps.slice(0, k * k).map((p) => h("span", {}, pimg(app, p.id, "t", "")))); })(),
    h("span", { class: "nm" }, t.name), h("span", { class: "n" }, `${fmt(t.n)} pictures`))));
  draw(group);
}

async function themePage(app, main, id, q, kind, alive) {
  fill(main, h("div", { class: "ax-ph" }, skel(3)), skelRow(24));
  const load = (page) => app.api(`/v1/theme/${enc(id)}?q=${enc(q)}&kind=${enc(kind)}&page=${page}`, { timeout: 60000 });
  const j = await load(0);
  if (!alive()) return;
  const t = j.theme;
  const grid = row(app, j.pictures);
  let page = 0;
  const btn = h("button", { type: "button", class: "ax-btn", onclick: async () => {
    btn.disabled = true;
    try { const m = await load(++page); grid.querySelector(":scope > i")?.remove(); grid.append(...(m.pictures || []).map((p) => tile(app, p)), h("i")); btn.style.display = m.more ? "" : "none"; } finally { btn.disabled = false; }
  } }, "More pictures");
  btn.style.display = j.more ? "" : "none";
  const qIn = h("input", { type: "search", class: "ix-in", value: q, placeholder: `Search inside ${t.name.toLowerCase()}`, "aria-label": "Search inside this theme" });
  qIn.addEventListener("keydown", (e) => { if (e.key === "Enter") app.go("theme", id, qIn.value.trim(), kind); });
  const kinds = Object.entries(t.kinds || {}).sort((a, b) => b[1] - a[1]).filter(([, n]) => n >= Math.max(20, t.n * 0.01)).slice(0, 7);
  const kindRow = h("div", { class: "ix-chips" }, h("a", { class: `ax-chip${kind ? "" : " f"}`, href: app.href("theme", id, q) }, "Every kind"),
    kinds.map(([k, n]) => h("a", { class: `ax-chip${kind === k ? " f" : ""}`, href: app.href("theme", id, q, k) }, `${KIND_WORDS[k] || k} · ${short(n)}`)));
  fill(main,
    h("div", { class: "ax-head", style: { marginTop: "10px" } },
      h("div", {}, h("div", { class: "ix-eyebrow" }, h("a", { href: app.href("wander") }, "Wander"), " → Theme"), h("h1", { class: "ix-h1" }, t.name),
        h("p", { class: "ix-sub" }, `${fmt(t.n)} pictures that look alike, found by grouping every picture's vector and named by hand from the words in their descriptions.${q ? ` Here the ones closest to “${q}”.` : " The ones closest to its middle come first."}`)),
      h("span", { class: "sp" }), (j.near || []).length ? [h("span", { class: "ix-note", style: { margin: 0 } }, "Next to it:"), j.near.map((n) => h("a", { class: "ax-chip", href: app.href("theme", n.id) }, n.name))] : null),
    h("div", { style: { display: "flex", gap: "10px", flexWrap: "wrap", alignItems: "center", margin: "8px 0 10px" } }, h("div", { style: { flex: "1 1 320px", maxWidth: "520px" } }, qIn), kindRow),
    (j.pictures || []).length ? grid : empty("No pictures of this kind in this theme."),
    h("div", { style: { display: "flex", gap: "8px", marginTop: "14px", justifyContent: "center", flexWrap: "wrap" } }, btn, h("a", { class: "ax-btn ghost", href: app.href("wander") }, "Back to the map")));
}

// ---------------------------------------------------------------- colors

const COLOR_KINDS = [["", "Any kind"], ["Painting", "Paintings"], ["Print,Illustration,Drawing", "Prints and drawings"], ["Photo,Old photo", "Photos"], ["Nature", "Nature"], ["Object", "Objects"]];
// The wheel: twelve hues at five lightnesses, then greys.
const WHEEL = (() => {
  const out = [];
  const hsl = (hh, s, l) => { const a = s * Math.min(l, 1 - l); const f = (n) => { const k = (n + hh / 30) % 12; return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)))); }; return `#${[f(0), f(8), f(4)].map((x) => x.toString(16).padStart(2, "0")).join("")}`; };
  for (const [s, l] of [[0.75, 0.82], [0.75, 0.66], [0.8, 0.5], [0.75, 0.36], [0.7, 0.22]]) for (let i = 0; i < 12; i++) out.push(hsl(i * 30, s, l));
  for (const l of [0.98, 0.85, 0.7, 0.55, 0.4, 0.28, 0.16, 0.06]) out.push(hsl(0, 0, l));
  out.push("#c8b48a", "#8b6b43", "#5a4630", "#d8cdb6");
  return out;
})();

async function colorsPage(app, main, arg, kindArg, alive) {
  let picks = arg.split(",").map((x) => x.trim().replace(/^#/, "")).filter((x) => /^[0-9a-f]{6}$/i.test(x)).slice(0, 3);
  let kind = COLOR_KINDS.some(([k]) => k === kindArg) ? kindArg : "";
  const picker = h("div", { class: "ix-picker" });
  const kindsRow = h("div", { style: { display: "flex", gap: "6px", flexWrap: "wrap", marginTop: "12px" } });
  const res = h("div", {});
  const wheel = h("div", { class: "ix-wheel" }, WHEEL.map((c) => h("button", { type: "button", style: { background: c }, "aria-label": `Pick ${c}`, title: c, onclick: () => { if (picks.length >= 3) picks.shift(); picks.push(c.slice(1)); go(); } })));
  const exact = h("input", { type: "color", value: "#1f3a6e", "aria-label": "Pick an exact color", onchange: () => { if (picks.length >= 3) picks.shift(); picks.push(exact.value.slice(1)); go(); } });
  const go = () => { app.put("colors", picks.join(","), kind); draw(); run(); };
  const draw = () => {
    fill(picker, picks.map((c, i) => h("button", { type: "button", class: "c on", style: { background: `#${c}` }, "aria-label": `#${c}, picked: remove it`, title: `#${c} · press to remove`, onclick: () => { picks.splice(i, 1); go(); } })),
      picks.length < 3 ? h("label", { class: "add", title: "Add an exact color" }, "+", exact) : null,
      h("span", { style: { fontSize: "13px", color: "var(--text-2)", marginLeft: "6px" } }, picks.length ? picks.map((c) => h("span", { class: "ix-hex" }, `#${c}`)) : "Pick a color from the grid"));
    fill(kindsRow, COLOR_KINDS.map(([k, t]) => h("button", { type: "button", class: `ax-chip${kind === k ? " f" : ""}`, onclick: () => { kind = k; go(); } }, t)));
  };
  let n = 0, page = 0;
  const run = async () => {
    if (!picks.length) { fill(res, empty("Pick up to three colors.")); return; }
    const me = ++n;
    page = 0;
    fill(res, skelRow(24));
    try {
      const j = await app.api(`/v1/colors?c=${picks.join(",")}&kind=${enc(kind)}`);
      if (me !== n || !alive()) return;
      const grid = row(app, j.pictures, "", { pal: true, cap: false });
      const btn = h("button", { type: "button", class: "ax-btn", onclick: async () => {
        btn.disabled = true;
        try { const m = await app.api(`/v1/colors?c=${picks.join(",")}&kind=${enc(kind)}&page=${++page}`); grid.querySelector(":scope > i")?.remove(); grid.append(...(m.pictures || []).map((p) => tile(app, p, { pal: true, cap: false })), h("i")); btn.style.display = m.more ? "" : "none"; } finally { btn.disabled = false; }
      } }, "More pictures");
      btn.style.display = j.more ? "" : "none";
      fill(res,
        h("div", { class: "ax-head" }, h("span", { class: "t" }, h("b", {}, picks.map((c) => `#${c}`).join(" and ")), " · each picture's main colors shown under it")),
        (j.pictures || []).length ? grid : empty("No picture holds these colors yet among those whose colors are known."),
        h("div", { style: { display: "flex", justifyContent: "center", marginTop: "18px" } }, btn),
        h("p", { class: "ix-note" }, `A picture's colors are measured from its copy on this site, so they are known for ${fmt(j.known)} of the ${fmt(j.total)} pictures so far: those shown at least once. The rest join as they are shown.`));
    } catch (e) { if (me === n) fill(res, h("div", { class: "ax-err" }, String(e.message || e))); }
  };
  fill(main,
    h("div", { style: { display: "flex", gap: "28px", flexWrap: "wrap", alignItems: "flex-start", margin: "10px 0 18px" } },
      h("div", { style: { flex: "1 1 420px", minWidth: "0" } }, h("h1", { class: "ix-h1" }, "Colors"),
        h("p", { class: "ix-sub", style: { maxWidth: "560px", margin: "4px 0 14px" } }, "Pick up to three colors. Pictures come back ranked by how much of each color they hold, measured the way people see color, so navy and orange finds harbours at dusk, rugs and travel posters alike."),
        picker, kindsRow),
      h("div", { style: { flex: "0 1 440px", minWidth: "0" } }, h("div", { class: "ix-eyebrow", style: { marginBottom: "6px" } }, "Pick a color"), wheel)),
    res);
  draw();
  run();
}

// ------------------------------------------------------------ how it's built

async function builtPage(app, main, alive) {
  fill(main, h("div", { class: "ax-grid two" }, h("article", { class: "ax-about" }, skel(12)), h("aside", {})));
  const j = await app.api("/v1/built");
  if (!alive()) return;
  const t = j.timings || {};
  const ms = (k, f) => (t[k] ? `${t[k][f].toFixed(1)} ms` : "not asked yet");
  const td = (x, mono) => h("td", { style: { padding: "8px 12px", fontFamily: mono ? "var(--mono)" : null, fontSize: mono ? "12.5px" : null } }, x);
  const table = (head, rows) => h("div", { class: "ix-table" }, h("table", {}, h("thead", {}, h("tr", {}, head.map((x) => h("th", {}, x)))), h("tbody", {}, rows.map((r) => h("tr", {}, r)))));
  const RECEIPTS = [["words", "Words (and steering)", j.pictures], ["facets", "Facets of the 2,000 closest", j.pictures], ["picture", "More like this", j.pictures], ["photo", "Your photo", j.pictures], ["detail", "A detail", j.pictures], ["mix", "A picture plus words", j.pictures], ["bridge", "Each step of a bridge", j.pictures], ["theme", "Inside a theme", null], ["colors", "Colors", j.colors]];
  const gb = (j.pictures * 768 * 4) / 1e9;
  fill(main, h("div", { class: "ax-grid two", style: { marginTop: "10px" } },
    h("article", { class: "ax-about" },
      h("h1", { style: { margin: "0 0 6px", fontSize: "27px", letterSpacing: "-.02em" } }, "How it's built"),
      h("p", {}, `Image Explorer is one ChloeDB database holding ${words(j.pictures)} pictures: each one's record, a CLIP vector of 768 numbers that says what is in it, and, once it has been shown, its main colors. Every search on the site is one SQL statement against that database. There is no other search engine behind it.`),
      h("h2", {}, "What is stored"),
      table(["Table", "Rows", "What is in it"], [
        [td("img_pictures", true), td(fmt(j.pictures)), td("collection, licence, size, the address of the file, a description written by a vision model, kind and theme (both guessed from the picture's vector), and the CLIP vector, indexed (HNSW)")],
        [td("img_colors", true), td(fmt(j.colors)), td("the six main colors of each picture shown so far and how much of it each covers, and a histogram of its colors as a vector of 144 numbers, made with its thumbnail")],
        [td("img_themes", true), td(fmt(j.themes)), td("the themes: k-means over the vectors, each one's middle, size, place on the map and neighbours; named by hand")],
        [td("img_links", true), td(fmt(j.commons_pages)), td("pages on Wikimedia Commons found for pictures from there, by the SHA-1 of the file")]]),
      h("p", {}, `The vectors are the heavy part: ${gb.toFixed(1)} GB as 32-bit numbers. Thumbnails are files beside the app, made the first time a picture is shown: ${fmt(j.thumbs_made)} made since it started, ${fmt(j.thumbs_failed)} could not be read.`),
      h("h2", {}, "One statement per search"),
      h("p", {}, `Words become a vector with the same CLIP model (ViT-L/14) that made the pictures' vectors, run in the app with ChloeDB's embedder (${j.embed?.text ? `${j.embed.text.median_ms} ms median for words${j.embed?.image ? `, ${j.embed.image.median_ms} ms for a photo or a detail` : ""}, on this machine's processor; words asked before are kept` : "timed once the site is used"}), and then:`),
      h("pre", {}, "SELECT id, title, source, licence, width, height, kind\nFROM img_pictures\nWHERE kind IN (:kinds)\nORDER BY similarity(clip, :v) DESC\nLIMIT 48"),
      h("p", {}, "Everything else on the site is that statement with a different ", h("code", {}, ":v"), ":"),
      h("ul", { style: { paddingLeft: "20px", margin: "6px 0" } },
        h("li", {}, h("b", {}, "Steering: "), "the words' vector moved toward the pictures you liked and away from the ones you hid."),
        h("li", {}, h("b", {}, "More like this: "), "the picture's own vector. The same picture in another collection is one that is nearly identical."),
        h("li", {}, h("b", {}, "Another medium: "), "the picture's vector plus a few words, like “a pencil drawing of”."),
        h("li", {}, h("b", {}, "A detail, your photo: "), "CLIP's vector for the box or the photo. The photo is not stored."),
        h("li", {}, h("b", {}, "A picture plus words: "), "the picture's vector plus the words', weighted by the slider."),
        h("li", {}, h("b", {}, "Bridge: "), "one statement for each step on the great circle between two vectors."),
        h("li", {}, h("b", {}, "More variety: "), "four times as many of the closest, then the ones least like those already picked (maximal marginal relevance)."),
        h("li", {}, h("b", {}, "Colors: "), h("code", {}, "ORDER BY similarity(colors, :c)"), " over img_colors, where each picture's colors are a histogram over CIE Lab, the space where equal distances look equally different, kept as square roots so that the score is how much two histograms overlap."),
        h("li", {}, h("b", {}, "Facets: "), "the kind, collection, licence, shape and size of the closest 2,000.")),
      h("p", {}, "The footer of each page has “The SQL behind this page”: the statements it ran, with their rows and times."),
      h("h2", {}, "Receipts"),
      h("p", {}, `How long each kind of statement took over its last ${fmt(Math.max(0, ...Object.values(t).map((x) => x?.n || 0)))} runs, measured here since the app started ${(() => { const m = Math.round((j.uptime_s || 0) / 60); return m < 2 ? "a minute ago" : m < 120 ? `${m} minutes ago` : `${Math.round(m / 60)} hours ago`; })()}, not estimated:`),
      table(["Search", "Median", "p99", "Pictures compared"], RECEIPTS.map(([k, l, c]) => [td(l), td(ms(k, "median_ms")), td(ms(k, "p99_ms")), td(c == null ? "one theme's" : fmt(c))])),
      h("h2", {}, "Where the pictures come from"),
      h("p", {}, `From PD12M, a set made by Spawning of 12.4 million pictures that are public domain or CC0, gathered from Wikimedia Commons, iNaturalist and hundreds of museums, archives and libraries, with a description and a CLIP vector for each. This site holds the first ${words(j.pictures)} of them. The files stay in PD12M's public storage; the site makes a thumbnail the first time a picture is shown and keeps it. Every picture keeps its licence and the name of the collection that holds it.`),
      h("h2", {}, "A public site, used fairly"),
      h("p", {}, "Searches are limited per visitor, there is no public SQL, uploaded photos are used once and thrown away, and the site asks nothing of the collections beyond what their licences already give.")),
    h("aside", { class: "ax-right", style: { display: "block" } },
      h("div", { class: "ax-card" }, h("h4", {}, "At a glance"),
        h("div", { class: "ax-kpis", style: { gridTemplateColumns: "repeat(2, minmax(0, 1fr))" } },
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, short(j.pictures)), h("div", { class: "l" }, "pictures")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, fmt(j.themes)), h("div", { class: "l" }, "themes")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, "768"), h("div", { class: "l" }, "numbers each")),
          h("div", { class: "ax-kpi" }, h("div", { class: "v" }, `${gb.toFixed(1)} GB`), h("div", { class: "l" }, "of vectors")))),
      h("div", { class: "ax-card" }, h("h4", {}, "The model"), h("p", { class: "ix-card-p" }, "CLIP ViT-L/14 (OpenAI), the model PD12M's vectors were made with, so words and photos land in the same space as the pictures. ", h("code", {}, j.model || ""))),
      h("div", { class: "ax-card" }, h("h4", {}, "Also built on ChloeDB"), h("p", { class: "ix-card-p" }, "Hacker News search at hn.chloetech.ai, the arXiv explorer at arxiv.chloetech.ai and the GitHub explorer at gh.chloetech.ai.")))));
}

export default { render };

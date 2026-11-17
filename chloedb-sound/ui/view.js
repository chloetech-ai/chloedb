// Sound Explorer on ChloeDB: the page, in ChloeDB's command center
// (#/app/sound, loaded through chloedb-server from this app) and on its own
// (the app's / with --public, through ui/index.html). It imports nothing
// and builds its own elements, so the same file serves both.
//
//   (home)               describe it, hum it or drop it; a few of each kind
//   search/<q>/<opts>    sounds by words and by how they sound, steered:
//                        closer to some, away from others, heavier, drier,
//                        "not thunder"; facets by source, kind and licence
//   sound/<id>           one recording: what is in it moment by moment, its
//                        credit line, sounds like it, more by its author
//   listen               a clip recorded or dropped (ten seconds at most):
//                        the sounds like it, and the recording it is cut from
//   moments/<q>          the ten seconds inside long recordings that match
//   scene/<q>            a place described, as layers to play together
//   map/<by>             every sound by how it sounds, or where recorded
//   music                the music shelf, and drifts from a song to another
//   drift/<from>/<to>    a path of sounds between two
//   about                how it works: the one page with ChloeDB's numbers
//
// Every answer of the API carries the SQL it ran: the "SQL" button shows
// the queries behind the page in view.

export default { render };

const CSS = `

.sx { color: var(--text); font: 14px/1.5 var(--sans); max-width: 1240px; margin: 0 auto; }
.sx * { box-sizing: border-box; }
.sx a { color: inherit; text-decoration: none; }
.sx a:hover { text-decoration: underline; text-underline-offset: 2px; }
.sx button { font: inherit; }
.sx mark { background: color-mix(in srgb, var(--warn) 34%, transparent); color: inherit; border-radius: 3px; padding: 0 1px; }
.sx svg.i { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; flex: none; }

/* header */
.sx-top { position: sticky; top: 0; z-index: 5; background: color-mix(in srgb, var(--bg) 88%, transparent); backdrop-filter: blur(10px); padding: 12px 0 6px; }
.sx-bar { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
.sx-brand { display: flex; align-items: center; gap: 9px; font-weight: 680; letter-spacing: -.015em; white-space: nowrap; font-size: 15.5px; }
.sx-brand svg { width: 26px; height: 26px; color: var(--accent); fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; }
.sx-brand small { font-weight: 500; color: var(--muted); font-size: 12px; }
.sx-box { position: relative; flex: 1 1 420px; min-width: 0; display: flex; align-items: center; }
.sx-box input { width: 100%; font: 15px var(--sans); padding: 10px 96px 10px 38px; border-radius: 11px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); outline: none; box-shadow: var(--shadow-1); }
.sx-box input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--focus); }
.sx-box > svg { position: absolute; left: 12px; top: 50%; width: 17px; height: 17px; transform: translateY(-50%); color: var(--muted); pointer-events: none; fill: none; stroke: currentColor; stroke-width: 2; }
.sx-box .acts { position: absolute; right: 6px; top: 50%; transform: translateY(-50%); display: flex; gap: 2px; }
.sx-ib { width: 34px; height: 34px; border-radius: 8px; border: 0; background: none; color: var(--text-2); display: inline-flex; align-items: center; justify-content: center; cursor: pointer; }
.sx-ib:hover { background: var(--accent-soft); color: var(--accent); }
.sx-ib.rec { color: var(--crit-ink); }
.sx-nav { display: flex; gap: 2px; margin-top: 8px; overflow-x: auto; scrollbar-width: none; align-items: center; }
.sx-nav a { padding: 5px 11px; border-radius: 8px; color: var(--text-2); font-size: 13px; white-space: nowrap; }
.sx-nav a:hover { background: var(--surface-2); text-decoration: none; }
.sx-nav a.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.sx-nav .sp { flex: 1; }
.sx-nav .live { display: inline-flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; padding: 5px 4px; white-space: nowrap; }
.sx-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--good); }
.sx-main { padding: 8px 0 40px; }

/* hero */
.sx-hero { text-align: center; padding: 40px 0 10px; }
.sx-hero h1 { margin: 0; font-size: 38px; letter-spacing: -.03em; line-height: 1.1; font-weight: 760; }
.sx-hero h1 span { color: var(--accent); }
.sx-hero p { color: var(--text-2); font-size: 16px; margin: 12px auto 0; max-width: 720px; }
.sx-ways { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px; margin: 26px auto 6px; max-width: 980px; }
@media (max-width: 820px) { .sx-ways { grid-template-columns: minmax(0, 1fr); } }
.sx-way { display: flex; gap: 12px; align-items: flex-start; text-align: left; padding: 14px 16px; border-radius: 12px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow-1); color: var(--text); cursor: pointer; }
.sx-way:hover { border-color: var(--accent); text-decoration: none !important; }
.sx-way .ic { width: 40px; height: 40px; border-radius: 10px; background: var(--accent-soft); color: var(--accent); display: inline-flex; align-items: center; justify-content: center; flex: none; }
.sx-way .ic.rec { background: var(--crit-soft); color: var(--crit-ink); }
.sx-way .ic svg { width: 20px; height: 20px; }
.sx-way b { display: block; font-size: 14.5px; font-weight: 650; }
.sx-way span { display: block; font-size: 13px; color: var(--text-2); margin-top: 2px; }
.sx-ex { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; margin: 18px 0 4px; }
@media (max-width: 1100px) { .sx-ex { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
.sx-ex .g { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 10px 12px; }
.sx-ex .g h5 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
.sx-ex .g a { display: block; font: 13px/1.45 var(--mono); color: var(--text); padding: 3px 0; }
.sx-ex .g a:hover { color: var(--accent); }

/* layout */
.sx-grid { display: grid; grid-template-columns: 220px minmax(0, 1fr) 300px; gap: 26px; align-items: start; }
.sx-grid.two { grid-template-columns: minmax(0, 1fr) 330px; }
@media (max-width: 1120px) { .sx-grid { grid-template-columns: 200px minmax(0, 1fr); } .sx-grid > .sx-right { display: none; } }
@media (max-width: 780px) { .sx-grid, .sx-grid.two { grid-template-columns: minmax(0, 1fr); } .sx-grid > .sx-left { display: none; } .sx-grid.two > .sx-right { display: block; } }
.sx-cols { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px; }
.sx-cols.four { grid-template-columns: repeat(4, minmax(0, 1fr)); }
.sx-cols.two { grid-template-columns: repeat(2, minmax(0, 1fr)); }
@media (max-width: 900px) { .sx-cols, .sx-cols.four { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 600px) { .sx-cols, .sx-cols.four, .sx-cols.two { grid-template-columns: minmax(0, 1fr); } }

/* chips, badges, buttons */
.sx-und { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin: 4px 0 12px; font-size: 12.5px; color: var(--muted); }
.sx-chip { display: inline-flex; align-items: center; gap: 5px; padding: 2px 9px; border-radius: 999px; background: var(--surface); border: 1px solid var(--line-2); color: var(--text-2); font-size: 12.5px; white-space: nowrap; cursor: pointer; line-height: 1.6; }
.sx-chip:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.sx-chip.f { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.sx-chip.t { background: color-mix(in srgb, var(--s7) 12%, var(--surface)); border-color: transparent; color: color-mix(in srgb, var(--s7) 80%, var(--text)); }
.sx-chip.n { background: var(--crit-soft); border-color: transparent; color: var(--crit-ink); }
.sx-chip .x { opacity: .6; font-size: 13px; line-height: 1; }
.sx-chip .from { color: var(--muted); font-family: var(--mono); font-size: 11px; }
.sx-badge { font-size: 10.5px; font-weight: 650; padding: 0 6px; border-radius: 5px; background: var(--surface-3); color: var(--text-2); letter-spacing: .03em; line-height: 1.75; white-space: nowrap; }
.sx-badge.cc0 { background: var(--good-soft); color: var(--good-ink); }
.sx-badge.by { background: var(--accent-soft); color: var(--accent); }
.sx-badge.nc { background: var(--warn-soft); color: var(--warn-ink); }
.sx-btn { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); cursor: pointer; font-size: 13px; white-space: nowrap; min-height: 34px; }
.sx-btn:hover { border-color: var(--accent); color: var(--accent); text-decoration: none !important; }
.sx-btn.pri { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); font-weight: 600; }
.sx-btn.pri:hover { background: var(--accent-2); color: var(--accent-ink); }
.sx-btn.rec { background: var(--crit); border-color: var(--crit); color: #fff; font-weight: 600; }
.sx-btn.on { background: var(--accent-soft); border-color: transparent; color: var(--accent); }
.sx-seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 8px; overflow: hidden; background: var(--surface); flex-wrap: wrap; }
.sx-seg button { font: 12.5px var(--sans); border: 0; background: none; color: var(--text-2); padding: 5px 11px; cursor: pointer; min-height: 30px; }
.sx-seg button + button { border-left: 1px solid var(--line); }
.sx-seg button.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
.sx-tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); margin: 6px 0 4px; overflow-x: auto; scrollbar-width: none; }
.sx-tabs a, .sx-tabs button { border: 0; background: none; padding: 8px 12px; color: var(--text-2); cursor: pointer; border-bottom: 2px solid transparent; white-space: nowrap; font-size: 13.5px; }
.sx-tabs .on { color: var(--accent); border-bottom-color: var(--accent); font-weight: 600; }
.sx-tabs .n { color: var(--muted); font-size: 12px; margin-left: 4px; }

/* sections and cards */
.sx-sec { display: flex; align-items: baseline; justify-content: space-between; margin: 26px 0 8px; gap: 10px; }
.sx-sec h3 { margin: 0; font-size: 12.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; }
.sx-sec a { font-size: 13px; color: var(--accent); }
.sx-sec .note { font-size: 12px; color: var(--muted); }
.sx-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; margin-bottom: 14px; box-shadow: var(--shadow-1); }
.sx-card h4 { margin: 0 0 8px; font-size: 11.5px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; display: flex; justify-content: space-between; gap: 8px; }
.sx-card h4 a { text-transform: none; letter-spacing: 0; color: var(--accent); font-weight: 500; }
.sx-card p { margin: 0 0 8px; font-size: 13px; color: var(--text-2); }
.sx-card .sub { font-size: 12.5px; color: var(--muted); }
.sx-facet { margin-bottom: 18px; }
.sx-facet h4 { margin: 0 0 6px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 600; }
.sx-facet label { display: flex; align-items: center; gap: 8px; padding: 3px 6px; margin: 0 -6px; border-radius: 6px; cursor: pointer; font-size: 13px; color: var(--text-2); }
.sx-facet label:hover { background: var(--surface-2); color: var(--text); }
.sx-facet label input { margin: 0; accent-color: var(--accent); }
.sx-facet label .n { margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; font-size: 12px; }
.sx-facet label .l { flex: 1; min-width: 0; }
.sx-facet label .l small { display: block; font-size: 11.5px; color: var(--muted); }

/* a sound in a list */
.sx-s { display: grid; grid-template-columns: 40px minmax(0, 1fr); gap: 4px 12px; padding: 13px 0; border-bottom: 1px solid var(--line); }
.sx-s:last-child { border-bottom: 0; }
.sx-play { width: 40px; height: 40px; border-radius: 50%; border: 1px solid var(--line-2); background: var(--surface); color: var(--accent); display: inline-flex; align-items: center; justify-content: center; cursor: pointer; padding: 0; }
.sx-play:hover { border-color: var(--accent); background: var(--accent-soft); }
.sx-play.on { background: var(--accent); border-color: var(--accent); color: var(--accent-ink); }
.sx-play svg { width: 16px; height: 16px; fill: currentColor; stroke: none; }
.sx-play.sm { width: 32px; height: 32px; }
.sx-play.sm svg { width: 13px; height: 13px; }
.sx-s .ttl { font-size: 15px; font-weight: 600; line-height: 1.35; letter-spacing: -.005em; }
.sx-s .by { font-size: 13px; color: var(--text-2); margin-top: 1px; }
.sx-s .by a:hover { color: var(--accent); }
.sx-s .meta { display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12.5px; color: var(--muted); margin-top: 4px; align-items: center; }
.sx-s .tags { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; }
.sx-s .why { font-size: 12px; color: var(--accent); margin-top: 5px; }
.sx-s .body { grid-column: 2; min-width: 0; }
.sx-s.compact { grid-template-columns: 32px minmax(0, 1fr); padding: 9px 0; gap: 2px 10px; }
.sx-s.compact .ttl { font-size: 13.5px; font-weight: 560; }
.sx-s.compact .by { font-size: 12px; }

/* waveforms */
.sx-wave { display: block; width: 100%; height: 44px; margin-top: 8px; }
.sx-wave.sm { height: 26px; margin-top: 4px; }
.sx-wave.lg { height: 120px; margin-top: 0; }
.sx-wave .b { stroke: var(--wave); stroke-width: 2; stroke-linecap: round; fill: none; }
.sx-wave .h { stroke: var(--wave-hi); stroke-width: 2; stroke-linecap: round; fill: none; }
.sx-wave .p { stroke: var(--wave-played); stroke-width: 2; stroke-linecap: round; fill: none; }
.sx-wave .band { fill: color-mix(in srgb, var(--accent) 9%, transparent); }
.sx-wave .tick { stroke: var(--s2); stroke-width: 2; }
.sx-time { display: flex; justify-content: space-between; font: 11px var(--mono); color: var(--muted); margin-top: 2px; }

/* tiles of sounds */
.sx-tile { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 12px 14px; box-shadow: var(--shadow-1); display: grid; grid-template-columns: 32px minmax(0, 1fr); gap: 2px 10px; align-items: start; }
.sx-tile .ttl { font-size: 13.5px; font-weight: 600; line-height: 1.35; }
.sx-tile .by { font-size: 12px; color: var(--muted); }
.sx-tile .w { grid-column: 1 / -1; }
.sx-tile .meta { grid-column: 1 / -1; display: flex; gap: 8px; align-items: center; font-size: 12px; color: var(--muted); margin-top: 4px; flex-wrap: wrap; }
.sx-tool { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 14px 16px; box-shadow: var(--shadow-1); display: block; color: var(--text); }
.sx-tool:hover { border-color: var(--accent); text-decoration: none !important; }
.sx-tool .ic { width: 34px; height: 34px; border-radius: 9px; background: var(--accent-soft); color: var(--accent); display: inline-flex; align-items: center; justify-content: center; margin-bottom: 8px; }
.sx-tool .ic svg { width: 18px; height: 18px; }
.sx-tool b { display: block; font-size: 14px; font-weight: 650; }
.sx-tool span { display: block; font-size: 13px; color: var(--text-2); margin-top: 3px; }
.sx-tool code { font: 12px var(--mono); color: var(--accent); display: block; margin-top: 8px; }

/* bars, key-values, credit */
.sx-bars { display: grid; grid-template-columns: 92px minmax(0, 1fr) 54px; gap: 6px 10px; align-items: center; font-size: 13px; }
.sx-bars .t { color: var(--text-2); }
.sx-bars .m { height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; }
.sx-bars .m span { display: block; height: 100%; background: var(--accent); border-radius: 3px; }
.sx-bars .v { color: var(--muted); font-size: 12px; text-align: right; font-variant-numeric: tabular-nums; }
.sx-kv { font-size: 13px; }
.sx-kv .row { display: flex; justify-content: space-between; gap: 10px; padding: 6px 0; border-top: 1px solid var(--line); }
.sx-kv .row:first-child { border-top: 0; }
.sx-kv .row span:first-child { color: var(--text-2); }
.sx-kv .row span:last-child { font-weight: 600; text-align: right; }
.sx-credit { font: 12px/1.55 var(--mono); background: var(--code); border: 1px solid var(--line); border-radius: 8px; padding: 9px 11px; color: var(--text); white-space: pre-wrap; word-break: break-word; margin: 8px 0; }
.sx-lic { display: flex; gap: 10px; align-items: flex-start; }
.sx-lic .cc { width: 36px; height: 36px; border-radius: 9px; background: var(--accent-soft); color: var(--accent); font: 700 12px var(--sans); display: inline-flex; align-items: center; justify-content: center; flex: none; }
.sx-lic b { display: block; font-size: 14px; }
.sx-lic span { display: block; font-size: 12.5px; color: var(--text-2); }

/* moments on a long recording */
.sx-mom { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
.sx-mom a, .sx-mom button { display: inline-flex; align-items: center; gap: 6px; padding: 3px 9px 3px 4px; border: 0; border-radius: 999px; background: color-mix(in srgb, var(--s2) 12%, var(--surface)); color: color-mix(in srgb, var(--s2) 62%, var(--text)); font-size: 12.5px; cursor: pointer; min-height: 28px; }
.sx-mom a:hover, .sx-mom button:hover { background: color-mix(in srgb, var(--s2) 22%, var(--surface)); text-decoration: none; }
.sx-mom a b, .sx-mom button b { font: 600 11.5px var(--mono); background: var(--surface); border-radius: 999px; padding: 0 6px; color: var(--text); }
.sx-seglbl { display: flex; gap: 4px; margin-top: 6px; }
.sx-seglbl span { flex: none; height: 22px; border-radius: 5px; font-size: 11.5px; line-height: 22px; padding: 0 6px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--text); }

/* mixer */
.sx-layer { display: grid; grid-template-columns: 150px minmax(0, 1fr) 150px 92px; gap: 12px; align-items: center; padding: 10px 0; border-top: 1px solid var(--line); }
.sx-layer:first-child { border-top: 0; }
@media (max-width: 900px) { .sx-layer { grid-template-columns: minmax(0, 1fr); } }
.sx-layer .nm { font-weight: 650; font-size: 13.5px; }
.sx-layer .nm small { display: block; font-weight: 400; color: var(--muted); font-size: 12px; }
.sx-layer .pick { min-width: 0; }
.sx-layer .pick .t { font-size: 13px; font-weight: 560; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sx-layer .pick .b { font-size: 12px; color: var(--muted); display: flex; gap: 6px; align-items: center; }
.sx-layer input[type=range] { width: 100%; accent-color: var(--accent); }
.sx-layer .sw { display: flex; gap: 4px; justify-content: flex-end; align-items: center; font-size: 12px; color: var(--muted); }
.sx-tl { position: relative; height: 22px; border-radius: 5px; background: var(--surface-2); margin: 3px 0; }
.sx-tl span { position: absolute; top: 3px; bottom: 3px; border-radius: 4px; }
.sx-tlax { display: flex; justify-content: space-between; font: 11px var(--mono); color: var(--muted); }

/* map */
.sx-map { position: relative; background: var(--surface); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; box-shadow: var(--shadow-1); }
.sx-map svg { display: block; width: 100%; height: auto; }
.sx-map .lb { position: absolute; transform: translate(-50%, -50%); font-size: 12.5px; font-weight: 650; color: var(--text); background: color-mix(in srgb, var(--surface) 82%, transparent); padding: 1px 7px; border-radius: 6px; white-space: nowrap; pointer-events: none; }
.sx-map .lb small { font-weight: 400; color: var(--muted); margin-left: 4px; }
.sx-map .ctl { position: absolute; left: 12px; top: 12px; display: flex; gap: 8px; flex-wrap: wrap; }
.sx-map .zoom { position: absolute; right: 12px; top: 12px; display: flex; flex-direction: column; gap: 4px; }
.sx-map .zoom button { width: 34px; height: 34px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); cursor: pointer; font-size: 16px; }
.sx-map .pin { position: absolute; width: 14px; height: 14px; border-radius: 50%; background: var(--s2); border: 2px solid #fff; transform: translate(-50%, -50%); box-shadow: 0 0 0 5px color-mix(in srgb, var(--s2) 25%, transparent); }

/* drift */
.sx-drift { display: grid; grid-template-columns: repeat(8, minmax(0, 1fr)); gap: 8px; align-items: stretch; }
@media (max-width: 1000px) { .sx-drift { grid-template-columns: repeat(4, minmax(0, 1fr)); } }
@media (max-width: 600px) { .sx-drift { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
.sx-step { background: var(--surface); border: 1px solid var(--line); border-radius: 9px; padding: 8px 9px; font-size: 12px; position: relative; }
.sx-step .k { font: 600 10.5px var(--mono); color: var(--muted); }
.sx-step b { display: block; font-size: 12.5px; font-weight: 600; line-height: 1.3; margin-top: 2px; }
.sx-step span { color: var(--muted); }
.sx-step.end { border-color: var(--accent); background: var(--accent-soft); }

/* recorder */
.sx-rec { background: var(--surface); border: 1px solid var(--line); border-radius: 14px; padding: 18px 20px; box-shadow: var(--shadow-1); }
.sx-rec .live { display: flex; align-items: center; gap: 14px; }
.sx-rec .big { width: 64px; height: 64px; border-radius: 50%; background: var(--crit); color: #fff; border: 0; display: inline-flex; align-items: center; justify-content: center; cursor: pointer; flex: none; box-shadow: 0 0 0 8px color-mix(in srgb, var(--crit) 16%, transparent); }
.sx-rec .big svg { width: 26px; height: 26px; }
.sx-rec .clock { font: 600 20px var(--mono); }
.sx-rec .hint { font-size: 13px; color: var(--text-2); }
.sx-drop { border: 1.5px dashed var(--line-2); border-radius: 12px; padding: 16px; text-align: center; color: var(--text-2); font-size: 13px; background: var(--surface-2); }
.sx-drop b { color: var(--text); }

/* how it works */
.sx-kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin: 16px 0; }
@media (max-width: 900px) { .sx-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
.sx-kpi { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 12px 14px; box-shadow: var(--shadow-1); }
.sx-kpi b { display: block; font-size: 24px; font-weight: 720; letter-spacing: -.02em; font-variant-numeric: tabular-nums; }
.sx-kpi span { display: block; font-size: 12.5px; color: var(--text-2); }
.sx-kpi small { display: block; font-size: 11.5px; color: var(--muted); margin-top: 4px; }
.sx-tbl-wrap { overflow-x: auto; }
.sx-tbl { width: 100%; border-collapse: collapse; font-size: 13px; }
.sx-tbl th { text-align: left; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); font-weight: 650; padding: 4px 10px 6px 0; border-bottom: 1px solid var(--line); }
.sx-tbl td { padding: 7px 10px 7px 0; border-bottom: 1px solid var(--line); vertical-align: top; }
.sx-tbl tr:last-child td { border-bottom: 0; }
.sx-tbl td.n { font-variant-numeric: tabular-nums; white-space: nowrap; }
.sx-tbl code, .sx-flow code { font: 12px var(--mono); }
.sx-flow { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; align-items: stretch; }
@media (max-width: 1000px) { .sx-flow { grid-template-columns: minmax(0, 1fr); } }
.sx-flow .st { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 12px 14px; font-size: 13px; color: var(--text-2); position: relative; }
.sx-flow .st b { display: block; color: var(--text); font-size: 13.5px; margin-bottom: 3px; }
.sx-flow .st .k { font: 600 10.5px var(--mono); color: var(--accent); display: block; margin-bottom: 4px; }
.sx-node { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); padding: 12px 14px; font-size: 12.5px; color: var(--text-2); }
.sx-node b { color: var(--text); font-size: 14px; }
.sx-meter { height: 6px; border-radius: 3px; background: var(--surface-3); overflow: hidden; margin: 4px 0 8px; }
.sx-meter span { display: block; height: 100%; background: var(--accent); }
.sx-meter.s span { background: var(--s3); }

/* sql pill and footer */
.sx-sql { display: flex; justify-content: flex-end; margin: 10px 0 0; }
.sx-sql button { font: 12px var(--mono); padding: 6px 11px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); cursor: pointer; box-shadow: var(--shadow-2); }
.sx-sql button b { color: var(--accent); }
.sx-sqlp { background: var(--surface); border: 1px solid var(--line-2); border-radius: 12px; box-shadow: var(--shadow-2); padding: 12px 14px; margin-top: 8px; }
.sx-sqlp pre { margin: 5px 0 0; font: 11.5px/1.5 var(--mono); white-space: pre-wrap; word-break: break-word; background: var(--code); padding: 8px 10px; border-radius: 7px; color: var(--text); }
.sx-sqlp .w { display: flex; justify-content: space-between; gap: 10px; font-size: 12.5px; color: var(--text-2); font-weight: 600; }
.sx-sqlp .w span { color: var(--muted); font-weight: 400; }
.sx-foot { color: var(--muted); font-size: 12px; text-align: center; padding: 26px 0 40px; border-top: 1px solid var(--line); margin-top: 26px; }
.sx-foot a { color: var(--accent); }
`;

const CSS2 = `
.sx { --wave: color-mix(in srgb, var(--text) 26%, transparent); --wave-hi: var(--accent); --wave-played: color-mix(in srgb, var(--accent) 70%, var(--text)); --warn-ink: color-mix(in srgb, var(--warn) 55%, var(--text)); }
.sx-wave { cursor: pointer; }
.sx-wave .p { stroke: var(--wave-hi); }
.sx-badge.by-sa, .sx-badge.fal { background: color-mix(in srgb, var(--s3) 16%, var(--surface)); color: color-mix(in srgb, var(--s3) 70%, var(--text)); }
.sx-badge.pd { background: var(--good-soft); color: var(--good-ink); }
.sx-skel { height: 64px; border-radius: 10px; background: linear-gradient(90deg, var(--surface-2), var(--surface-3), var(--surface-2)); background-size: 200% 100%; animation: sxsk 1.2s infinite; margin: 10px 0; }
@keyframes sxsk { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
.sx-empty { color: var(--muted); padding: 30px 0; text-align: center; }
.sx-err { color: var(--crit-ink); background: var(--crit-soft); border-radius: 9px; padding: 10px 12px; margin: 10px 0; font-size: 13px; }
.sx-sqlf { position: fixed; right: 18px; bottom: 16px; z-index: 20; }
.sx-sqlf > button { font: 12px var(--mono); padding: 6px 11px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text-2); cursor: pointer; box-shadow: var(--shadow-2); }
.sx-sqlf > button b { color: var(--accent); }
.sx-sqld { position: fixed; right: 18px; bottom: 56px; width: min(760px, calc(100vw - 36px)); max-height: 72vh; overflow: auto; z-index: 21; display: none; }
.sx-sqld.on { display: block; }
.sx-sqld .q { border-top: 1px solid var(--line); padding: 9px 0; }
.sx-sqld .q:first-of-type { border-top: 0; }
.sx-sqld .bad { color: var(--crit-ink); font-size: 12px; }
.sx-steer { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin: 2px 0 10px; }
.sx-steer .lbl { font-size: 12px; color: var(--muted); margin-right: 2px; }
.sx-steer input { font: 13px var(--sans); padding: 5px 9px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); width: 150px; }
.sx-s .acts { display: flex; gap: 6px; margin-top: 6px; flex-wrap: wrap; }
.sx-s .acts button { font: 12px var(--sans); border: 1px solid var(--line); background: var(--surface); color: var(--text-2); border-radius: 7px; padding: 2px 8px; cursor: pointer; min-height: 26px; }
.sx-s .acts button:hover { border-color: var(--accent); color: var(--accent); }
.sx-s .acts button.on { background: var(--accent-soft); color: var(--accent); border-color: transparent; }
.sx-tl span { opacity: .85; }
.sx-tlrow { display: grid; grid-template-columns: 150px minmax(0, 1fr) 46px; gap: 10px; align-items: center; font-size: 13px; }
.sx-tlrow .v { font: 11.5px var(--mono); color: var(--muted); text-align: right; }
.sx-canvas { display: block; width: 100%; height: 620px; touch-action: none; cursor: crosshair; }
.sx-tip { position: absolute; pointer-events: none; background: var(--surface); border: 1px solid var(--line-2); border-radius: 8px; padding: 6px 9px; font-size: 12.5px; box-shadow: var(--shadow-2); max-width: 280px; display: none; z-index: 3; }
.sx-legend { display: flex; flex-wrap: wrap; gap: 10px; font-size: 12.5px; color: var(--text-2); margin: 8px 0; }
.sx-legend i { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: 5px; vertical-align: -1px; }
.sx-meterbar { height: 8px; border-radius: 4px; background: var(--surface-3); overflow: hidden; flex: 1; }
.sx-meterbar span { display: block; height: 100%; background: var(--crit); width: 0; transition: width .08s; }
.sx-orig { border: 1px solid var(--accent); background: var(--accent-soft); border-radius: 12px; padding: 12px 14px; margin: 12px 0; }
.sx-orig h4 { margin: 0 0 4px; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; color: var(--accent); }
.sx-mix { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin: 8px 0 4px; }
.sx-toast { position: fixed; left: 50%; bottom: 22px; transform: translateX(-50%); background: var(--text); color: var(--bg); border-radius: 9px; padding: 8px 14px; font-size: 13px; z-index: 30; opacity: 0; transition: opacity .2s; pointer-events: none; }
.sx-toast.on { opacity: .95; }
.sx-pick { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.sx-pick input { font: 13px var(--sans); padding: 6px 9px; border-radius: 8px; border: 1px solid var(--line-2); background: var(--surface); color: var(--text); min-width: 220px; }
.sx-drift { grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); }
.sx-step { cursor: pointer; }
.sx-step.on { border-color: var(--accent); box-shadow: 0 0 0 2px var(--focus); }
@media (max-width: 1000px) { .sx-grid, .sx-grid.two { grid-template-columns: minmax(0, 1fr); } .sx-left, .sx-right { display: none; } .sx-cols, .sx-cols.four, .sx-ex, .sx-ways, .sx-kpis, .sx-flow { grid-template-columns: repeat(2, minmax(0, 1fr)); } .sx-layer { grid-template-columns: 110px minmax(0, 1fr); } .sx-layer .sw, .sx-layer input { grid-column: 2; } }
@media (max-width: 600px) { .sx-cols, .sx-cols.four, .sx-cols.two, .sx-ex, .sx-ways, .sx-kpis, .sx-flow { grid-template-columns: minmax(0, 1fr); } .sx-hero h1 { font-size: 28px; } .sx-canvas { height: 440px; } .sx-tlrow { grid-template-columns: 100px minmax(0, 1fr) 40px; } }
`;

// --- small helpers -----------------------------------------------------------

const SVG = new Set(["svg", "g", "line", "circle", "path", "text", "rect", "polyline", "clipPath", "defs"]);
function h(tag, attrs, ...kids) {
  const el = SVG.has(tag) ? document.createElementNS("http://www.w3.org/2000/svg", tag) : document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "class") el.setAttribute("class", v);
    else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? "" : v);
  }
  add(el, kids);
  return el;
}
function fill(el, ...kids) { el.replaceChildren(); return add(el, kids); }
function add(el, kids) {
  for (const k of kids.flat(9)) {
    if (k == null || k === false) continue;
    el.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
  return el;
}
const enc = encodeURIComponent;
const fmt = (n) => (n == null || isNaN(n) ? "–" : Number(n).toLocaleString("en-US"));
function clock(s) {
  s = Math.max(0, Math.round(s || 0));
  const hh = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return hh ? `${hh}:${String(m).padStart(2, "0")}:${String(x).padStart(2, "0")}` : `${m}:${String(x).padStart(2, "0")}`;
}
const ICONS = {
  logo: '<path d="M3 12h1.5M7 9v6M10.5 5v14M14 8v8M17.5 10.5v3M21 12h-.5"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.6-3.6"/>',
  mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
  up: '<path d="M12 16V4M7 9l5-5 5 5M5 20h14"/>',
  play: '<path d="M8 5.5v13l11-6.5z"/>',
  pause: '<path d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z"/>',
  stop: '<rect x="7" y="7" width="10" height="10" rx="1.5"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>',
  link: '<path d="M10 14a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1M14 10a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1"/>',
  layers: '<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>',
  map: '<path d="M9 4 3 6v14l6-2 6 2 6-2V4l-6 2z"/><path d="M9 4v14M15 6v14"/>',
  clock: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>',
  drift: '<path d="M4 17c4 0 4-10 8-10s4 10 8 10"/><circle cx="4" cy="17" r="1.6"/><circle cx="20" cy="17" r="1.6"/>',
  music: '<path d="M9 18V5l11-2v13"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>',
};
function icon(name, cls = "i") {
  const s = h("svg", { class: cls, viewBox: "0 0 24 24", "aria-hidden": "true" });
  s.innerHTML = ICONS[name];
  return s;
}
function playIcon(on) {
  const s = h("svg", { viewBox: "0 0 24 24", "aria-hidden": "true" });
  s.innerHTML = on ? ICONS.pause : ICONS.play;
  return s;
}
const KIND = { effect: "Sound effect", music: "Music", nature: "Nature", animal: "Animal", voice: "Voice", place: "Place" };
const KCOLOR = { effect: "--s1", music: "--s7", nature: "--s3", animal: "--s2", voice: "--s5", place: "--s4" };
const LIC = { cc0: "CC0", pd: "Public domain", by: "CC BY", "by-sa": "CC BY-SA", fal: "Free Art", gfdl: "GFDL", other: "Free licence" };
const SRC = { fsd50k: "Freesound (FSD50K)", commons: "Wikimedia Commons", fma: "Free Music Archive" };
const STEER = [["heavier", "Heavier"], ["lighter", "Lighter"], ["drier", "Drier"], ["wetter", "More echo"], ["closer", "Closer"], ["farther", "Farther"], ["busier", "Busier"], ["calmer", "Calmer"]];
const EXAMPLES = {
  "Places": ["a busy harbour at dawn", "rain on a tin roof", "a quiet forest with a stream", "a train station announcement"],
  "Things": ["door creak", "glass breaking", "keys jingling", "a typewriter"],
  "Animals": ["a dog barking far away", "birds singing in the morning", "frogs at night", "a rooster"],
  "Music": ["a slow piano melody", "drums and bass", "an acoustic guitar song", "choir singing"],
  "Inside long recordings": ["thunder", "church bells", "applause", "a car passing by"],
};
function cssVar(name, el) { return getComputedStyle(el || document.documentElement).getPropertyValue(name).trim() || "#888"; }

// --- the player ----------------------------------------------------------------

// One sound plays at a time (a scene's layers apart). The page on its own
// plays a preview from its URL; in the command center, whose requests carry
// the token in a header, it is fetched as JSON and played from a blob.
function player(app) {
  const audio = new Audio();
  audio.preload = "none";
  const blobs = new Map();
  const p = { id: null, audio, listeners: new Set() };
  async function src(id) {
    if (app.ctx.media) return app.ctx.media(`/v1/audio/${enc(id)}`);
    if (blobs.has(id)) return blobs.get(id);
    const j = await app.ctx.api(`/v1/audio/${enc(id)}?b64=1`, { timeout: 60000 });
    const bin = atob(j.data);
    const a = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
    const u = URL.createObjectURL(new Blob([a], { type: j.mime }));
    if (blobs.size > 60) { const [k, v] = blobs.entries().next().value; URL.revokeObjectURL(v); blobs.delete(k); }
    blobs.set(id, u);
    return u;
  }
  p.play = async (id, at) => {
    if (p.id === id && at == null) {
      if (audio.paused) await audio.play().catch(() => {}); else audio.pause();
      return p.emit();
    }
    p.id = id;
    p.emit();
    try {
      audio.src = await src(id);
      if (p.id !== id) return;
      if (at != null) {
        await new Promise((res) => { audio.addEventListener("loadedmetadata", res, { once: true }); audio.load(); });
        audio.currentTime = at;
      }
      await audio.play();
    } catch (e) { app.toast(`Could not play it: ${e.message || e}`); }
    p.emit();
  };
  p.seek = (id, frac, dur) => {
    if (p.id !== id) return p.play(id, frac * dur);
    if (audio.duration) audio.currentTime = frac * audio.duration;
  };
  p.stop = () => { audio.pause(); p.id = null; p.emit(); };
  p.playing = (id) => p.id === id && !audio.paused;
  p.emit = () => { for (const f of p.listeners) f(); };
  for (const ev of ["play", "pause", "ended", "timeupdate"]) audio.addEventListener(ev, () => p.emit());
  return p;
}

// A play button for a sound, that follows the player.
function playBtn(app, s, opts = {}) {
  const b = h("button", { type: "button", class: `sx-play${opts.sm ? " sm" : ""}`, "aria-label": `Play ${s.title}` });
  const draw = () => {
    const on = app.player.playing(s.id);
    b.classList.toggle("on", on);
    b.setAttribute("aria-label", `${on ? "Pause" : "Play"} ${s.title}`);
    fill(b, playIcon(on));
  };
  b.addEventListener("click", () => app.player.play(s.id, opts.at));
  app.watch(b, draw);
  return b;
}

// A waveform from a sound's peaks: the part played in the accent colour,
// bands for moments, a click to play from there.
function wave(app, s, opts = {}) {
  const peaks = String(s.peaks || "").split(",").map(Number).filter((x) => !isNaN(x));
  const n = opts.n || (opts.cls === "sm" ? 80 : 160);
  const vals = resample(peaks.length ? peaks : [10], n);
  const H = opts.cls === "lg" ? 120 : opts.cls === "sm" ? 26 : 44;
  const svg = h("svg", { class: `sx-wave${opts.cls ? " " + opts.cls : ""}`, viewBox: `0 0 300 ${H}`, preserveAspectRatio: "none", role: "img", "aria-label": `Waveform of ${s.title}; click to play from a point` });
  const dur = s.seconds || 1;
  for (const b of opts.bands || []) {
    svg.append(h("rect", { class: "band", x: (b[0] / dur) * 300, y: 0, width: Math.max(2, ((b[1] - b[0]) / dur) * 300), height: H }));
  }
  const rest = h("path", { class: "b", "vector-effect": "non-scaling-stroke" });
  const done = h("path", { class: "p", "vector-effect": "non-scaling-stroke" });
  svg.append(rest, done);
  const bar = (i) => {
    const x = ((i + 0.5) * 300) / n, y = Math.max(0.6, (vals[i] / 100) * (H / 2 - 1));
    return `M${x.toFixed(1)} ${(H / 2 - y).toFixed(1)}V${(H / 2 + y).toFixed(1)}`;
  };
  let last = -2;
  const draw = () => {
    const a = app.player.audio;
    const k = app.player.id === s.id && a.duration ? Math.round((a.currentTime / a.duration) * n) : -1;
    if (k === last) return;
    last = k;
    let d1 = "", d2 = "";
    for (let i = 0; i < n; i++) (i < k ? (d2 += bar(i)) : (d1 += bar(i)));
    rest.setAttribute("d", d1);
    done.setAttribute("d", d2);
  };
  svg.addEventListener("click", (e) => {
    const r = svg.getBoundingClientRect();
    app.player.seek(s.id, Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), dur);
  });
  app.watch(svg, draw);
  return svg;
}
function resample(a, n) {
  if (a.length === n) return a;
  const out = [];
  for (let i = 0; i < n; i++) {
    const lo = Math.floor((i * a.length) / n), hi = Math.max(lo + 1, Math.floor(((i + 1) * a.length) / n));
    let m = 0;
    for (let j = lo; j < hi && j < a.length; j++) m = Math.max(m, a[j]);
    out.push(m);
  }
  return out;
}

function badge(s) { return h("span", { class: `sx-badge ${s.lic || ""}`, title: s.licence || "" }, LIC[s.lic] || s.licence || "licence"); }

// A sound in a list.
function row(app, s, opts = {}) {
  const meta = h("div", { class: "meta" }, badge(s), h("span", {}, SRC[s.source] || s.source), h("span", {}, clock(s.seconds)), s.kind ? h("span", {}, KIND[s.kind] || s.kind) : null, s.year ? h("span", {}, String(s.year)) : null,
    opts.score != null ? h("span", { title: "CLAP's similarity of this sound to what was asked" }, `match ${(opts.score * 100).toFixed(0)}`) : null);
  const body = h("div", { class: "body" },
    h("a", { class: "ttl", href: app.href("sound", s.id) }, s.title),
    h("div", { class: "by" }, s.author ? ["by ", s.author_url ? h("a", { href: s.author_url, target: "_blank", rel: "noopener" }, s.author) : s.author] : null),
    meta,
    opts.moments ? h("div", { class: "sx-mom" }, opts.moments.map((m) => h("button", { type: "button", onclick: () => app.player.play(s.id, m.t), title: `Play from ${clock(m.t)}` }, h("b", {}, clock(m.t)), `${(m.score * 100).toFixed(0)}`))) : null,
    wave(app, s, { cls: opts.big ? "" : "sm", bands: opts.bands }),
    opts.acts ? h("div", { class: "acts" }, opts.acts) : null);
  return h("div", { class: `sx-s${opts.big ? "" : " compact"}` }, playBtn(app, s, { sm: !opts.big }), body);
}

function skel(n) { return Array.from({ length: n }, () => h("div", { class: "sx-skel" })); }
function errBox(e) { return h("div", { class: "sx-err" }, String(e && e.message ? e.message : e)); }

// --- the page --------------------------------------------------------------------

function render(el, ctx) {
  if (!document.getElementById("sx-css")) document.head.append(h("style", { id: "sx-css" }, CSS + CSS2));
  const watchers = new Set();
  const app = {
    ctx,
    queries: [],
    took: 0,
    seq: 0,
    api: async (path, opts) => {
      const j = await ctx.api(path, opts);
      if (j && Array.isArray(j.queries)) { app.queries = j.queries; app.took = j.took_ms; drawSql(); }
      return j;
    },
    add: (j, part) => {
      if (j && Array.isArray(j.queries)) { app.queries = app.queries.concat(j.queries.map((q) => ({ ...q, part }))); drawSql(); }
      return j;
    },
    href: (...parts) => ctx.href(...parts),
    go: (...parts) => { location.hash = ctx.href(...parts); },
    // A function to call when the player changes, while its element is on the page.
    watch: (node, f) => { watchers.add([node, f]); f(); },
    toast: (m) => { fill(toast, m); toast.classList.add("on"); clearTimeout(app._t); app._t = setTimeout(() => toast.classList.remove("on"), 2600); },
  };
  app.player = player(app);
  app.player.listeners.add(() => {
    for (const w of [...watchers]) { if (!w[0].isConnected) watchers.delete(w); else w[1](); }
  });

  const root = h("div", { class: "sx" });
  fill(el, root);
  const toast = h("div", { class: "sx-toast", role: "status" });

  // The bar: the box, the mic and the file, and the pages.
  const input = h("input", { type: "search", placeholder: "Describe a sound: rain on a tin roof, a door creaking, birds at dawn", autocomplete: "off", spellcheck: "false", "aria-label": "Describe a sound" });
  const file = h("input", { type: "file", accept: "audio/*", style: { display: "none" }, onchange: () => { if (file.files[0]) { app.pendingFile = file.files[0]; file.value = ""; app.go("listen", "file"); } } });
  const box = h("div", { class: "sx-box" }, icon("search"), input,
    h("span", { class: "acts" },
      h("a", { class: "sx-ib rec", href: app.href("listen"), "aria-label": "Record or hum a sound", title: "Record or hum a sound" }, icon("mic")),
      h("button", { type: "button", class: "sx-ib", "aria-label": "Drop a sound file", title: "Search with a sound file", onclick: () => file.click() }, icon("up"))));
  const nav = h("nav", { class: "sx-nav", "aria-label": "Sections" });
  const top = h("div", { class: "sx-top" }, h("div", { class: "sx-bar" }, h("a", { class: "sx-brand", href: app.href("") }, icon("logo", ""), "Sound Explorer", h("small", {}, "on ChloeDB")), box, file), nav);
  const main = h("main", { class: "sx-main" });
  const sqlBtn = h("button", { type: "button", onclick: () => sqlPanel.classList.toggle("on"), title: "The SQL behind this page" });
  const sqlPanel = h("div", { class: "sx-sqld sx-sqlp" });
  root.append(top, main, h("div", { class: "sx-sqlf" }, sqlBtn), sqlPanel, toast,
    h("div", { class: "sx-foot" }, "Sounds and music stay their authors', under their licences, each credited and linked to its source: Wikimedia Commons, Freesound (through FSD50K) and the Free Music Archive. Not affiliated with any of them. ",
      h("a", { href: app.href("about") }, "How it works")));
  input.addEventListener("keydown", (e) => { if (e.key === "Enter" && input.value.trim()) app.go("search", input.value.trim()); });
  // A sound file dropped anywhere on the page is searched with.
  root.addEventListener("dragover", (e) => { if ([...(e.dataTransfer?.items || [])].some((i) => i.kind === "file")) e.preventDefault(); });
  root.addEventListener("drop", (e) => { const f = e.dataTransfer?.files?.[0]; if (f) { e.preventDefault(); app.pendingFile = f; app.go("listen", "file"); } });

  function drawSql() {
    const ms = app.queries.reduce((a, q) => a + (q.ms || 0), 0);
    fill(sqlBtn, "SQL · ", h("b", {}, String(app.queries.length)), ` ${app.queries.length === 1 ? "query" : "queries"} · ${Math.round(app.took || ms)} ms`);
    fill(sqlPanel, h("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "6px" } },
      h("b", {}, "The SQL behind this page"), h("button", { type: "button", class: "sx-btn", onclick: () => sqlPanel.classList.remove("on") }, "Close")),
      app.queries.map((q) => h("div", { class: "q" },
        h("div", { class: "w" }, q.part ? h("i", {}, q.part + " · ") : null, q.what, h("span", {}, `${fmt(q.rows)} rows · ${(q.ms ?? 0).toFixed(1)} ms`, q.server_ms != null ? ` (server ${q.server_ms.toFixed(1)} ms)` : "")),
        q.note ? h("div", { class: "bad" }, q.note) : null,
        h("pre", {}, q.sql, q.params && Object.keys(q.params).length ? `\n-- ${JSON.stringify(shortParams(q.params))}` : ""))));
  }
  drawSql();

  const PAGES = [["", "Search"], ["listen", "Listen"], ["moments", "Moments"], ["scene", "Scenes"], ["map", "Map"], ["music", "Music"], ["about", "How it works"]];
  function drawNav(page) {
    fill(nav, ...PAGES.map(([p, t]) => h("a", { href: app.href(p), class: page === p || (p === "" && (page === "search" || page === "sound")) ? "on" : "" }, t)));
  }

  let current = null;
  async function show(path) {
    current = path;
    const [page = "", ...rest] = path;
    const my = ++app.seq;
    const alive = () => my === app.seq;
    drawNav(page);
    app.queries = []; app.took = 0; drawSql();
    sqlPanel.classList.remove("on");
    if (page === "search" && rest[0] != null && input.value !== rest[0]) input.value = rest[0];
    if (page !== "search") input.value = page === "moments" || page === "scene" ? (rest[0] || "") : "";
    fill(main, skel(4));
    window.scrollTo({ top: 0 });
    try {
      const pages = { "": home, search, sound: soundPage, listen, moments, scene, map, music, drift, about };
      await (pages[page] || home)(main, app, rest, alive);
    } catch (e) { if (alive()) fill(main, errBox(e)); }
  }
  ctx.onPath((p) => { if (JSON.stringify(p) !== JSON.stringify(current)) show(p); });
  show(ctx.path() || []);
}

function shortParams(p) {
  const o = {};
  for (const [k, v] of Object.entries(p)) o[k] = Array.isArray(v) && v.length > 16 && typeof v[0] === "number" ? `[${v.length} numbers: a vector]` : v;
  return o;
}

// --- home --------------------------------------------------------------------------

async function home(main, app, _rest, alive) {
  const way = (href, ic, cls, b, s) => h("a", { class: "sx-way", href }, h("span", { class: `ic ${cls}` }, icon(ic)), h("div", {}, h("b", {}, b), h("span", {}, s)));
  const shelves = h("div", {}, skel(3));
  fill(main,
    h("div", { class: "sx-hero" },
      h("h1", {}, "Find a sound by ", h("span", {}, "how it sounds")),
      h("p", {}, "Describe it, hum it, or drop a clip. Free sounds and music from Wikimedia Commons, Freesound and the Free Music Archive, each with its credit line and licence.")),
    h("div", { class: "sx-ways" },
      way(app.href("search", "rain on a tin roof"), "search", "", "Describe it", "Words find sounds by how they sound, not only by their titles."),
      way(app.href("listen"), "mic", "rec", "Hum or record it", "Ten seconds into the mic; the clip is heard and dropped."),
      way(app.href("moments", "thunder"), "clock", "", "Find the moment", "The ten seconds inside a long recording, and play from there.")),
    h("div", { class: "sx-ex" }, Object.entries(EXAMPLES).map(([g, xs]) => h("div", { class: "g" }, h("h5", {}, g), xs.map((x) => h("a", { href: g.startsWith("Inside") ? app.href("moments", x) : app.href("search", x) }, x))))),
    shelves);
  const j = await app.api("/v1/home");
  if (!alive()) return;
  fill(shelves, j.shelves.map((sh) => [
    h("div", { class: "sx-sec" }, h("h3", {}, sh.title), h("a", { href: app.href("search", "", `kind=${sh.kind}`) }, "More")),
    h("div", { class: "sx-cols" }, sh.sounds.map((s) => tile(app, s))),
  ]));
}

function tile(app, s) {
  return h("div", { class: "sx-tile" }, playBtn(app, s, { sm: true }),
    h("div", {}, h("a", { class: "ttl", href: app.href("sound", s.id) }, s.title), h("div", { class: "by" }, [s.author, clock(s.seconds)].filter(Boolean).join(" · "))),
    h("div", { class: "w" }, wave(app, s, { cls: "sm", n: 60 })),
    h("div", { class: "meta" }, badge(s), h("span", {}, SRC[s.source] || s.source)));
}

// --- search ------------------------------------------------------------------------

function parseOpts(s) {
  const o = {};
  for (const [k, v] of new URLSearchParams(s || "")) o[k] = v;
  return o;
}
function optsString(o) {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v != null && v !== "") p.set(k, v);
  return p.toString();
}
const listOf = (s) => (s ? s.split(",").filter(Boolean) : []);
function toggle(list, x) { return list.includes(x) ? list.filter((y) => y !== x) : list.concat([x]); }

async function search(main, app, rest, alive) {
  const q = rest[0] || "";
  const o = parseOpts(rest[1]);
  const go = (patch) => { const n = { ...o, ...patch }; delete n.page; app.go("search", q, optsString(n)); };
  const left = h("aside", { class: "sx-left" });
  const body = h("div", {}, skel(5));
  const right = h("aside", { class: "sx-right" });
  const not = h("input", { type: "search", placeholder: "not… (e.g. thunder)", value: o.not || "", "aria-label": "What should not be in it", onkeydown: (e) => { if (e.key === "Enter") go({ not: not.value.trim() }); } });
  const steer = listOf(o.steer);
  const bySeg = h("div", { class: "sx-seg", role: "group", "aria-label": "Rank by" },
    [["", "Sound and words"], ["sound", "How it sounds"], ["words", "Words only"]].map(([k, t]) => h("button", { type: "button", class: (o.by || "") === k ? "on" : "", onclick: () => go({ by: k }) }, t)));
  const head = h("div", {},
    h("div", { class: "sx-steer" }, h("span", { class: "lbl" }, "Steer"), STEER.map(([k, t]) => h("button", { type: "button", class: `sx-chip${steer.includes(k) ? " f" : ""}`, onclick: () => go({ steer: toggle(steer, k).join(",") }) }, t)), not),
    h("div", { class: "sx-steer" }, h("span", { class: "lbl" }, "Rank by"), bySeg));
  fill(main, h("div", { class: "sx-grid" }, left, h("div", {}, head, body), right));
  const params = new URLSearchParams({ q, page: o.page || 0 });
  for (const k of ["not", "like", "unlike", "steer", "source", "kind", "lic", "min", "max", "by"]) if (o[k]) params.set(k, o[k]);
  const j = await app.api(`/v1/search?${params}`);
  if (!alive()) return;
  const like = listOf(o.like), unlike = listOf(o.unlike);
  const und = h("div", { class: "sx-und" }, `${fmt(j.total)}${j.total >= 200 ? "+" : ""} sounds, ranked by ${{ both: "how they sound and their words", sound: "how they sound", words: "their words", browse: "length" }[j.by] || j.by}`,
    j.understood.filter((u) => u.kind !== "words").map((u) => h("span", { class: `sx-chip ${u.kind === "not" || u.kind === "unlike" ? "n" : "t"}` }, { like: "closer to a sound", unlike: "away from a sound", not: `not ${u.text}`, steer: u.text, note: u.text }[u.kind] || u.kind)),
    (like.length || unlike.length) ? h("button", { type: "button", class: "sx-chip", onclick: () => go({ like: "", unlike: "" }) }, "forget the sounds steered by ×") : null);
  const results = j.results.map((s) => row(app, s, {
    big: true, score: j.by === "words" ? null : s.score,
    acts: [
      h("button", { type: "button", class: like.includes(s.id) ? "on" : "", onclick: () => go({ like: toggle(like, s.id).join(","), unlike: unlike.filter((x) => x !== s.id).join(",") }), title: "Move the search towards this sound" }, "More like this"),
      h("button", { type: "button", class: unlike.includes(s.id) ? "on" : "", onclick: () => go({ unlike: toggle(unlike, s.id).join(","), like: like.filter((x) => x !== s.id).join(",") }), title: "Move the search away from this sound" }, "Less like this"),
    ],
  }));
  const page = Number(o.page || 0);
  fill(body, und, results.length ? results : h("div", { class: "sx-empty" }, "Nothing found: try other words, or take a filter off."),
    h("div", { style: { display: "flex", gap: "8px", margin: "14px 0" } },
      page > 0 ? h("a", { class: "sx-btn", href: app.href("search", q, optsString({ ...o, page: page - 1 })) }, "Previous") : null,
      j.more ? h("a", { class: "sx-btn", href: app.href("search", q, optsString({ ...o, page: page + 1 })) }, "Next") : null));
  const facet = (title, col, names) => {
    const on = listOf(o[col]);
    const vals = (j.facets[col] || []);
    for (const v of on) if (!vals.some((x) => x.value === v)) vals.push({ value: v, n: 0 });
    return vals.length ? h("div", { class: "sx-facet" }, h("h4", {}, title), vals.map((v) => h("label", {},
      h("input", { type: "checkbox", checked: on.includes(v.value), onchange: () => go({ [col]: toggle(on, v.value).join(",") }) }),
      h("span", { class: "l" }, names[v.value] || v.value), h("span", { class: "n" }, fmt(v.n))))) : null;
  };
  const len = h("div", { class: "sx-facet" }, h("h4", {}, "Length"),
    [["", "", "Any"], ["", "10", "Up to 10 s"], ["10", "60", "10 s to a minute"], ["60", "", "Over a minute"]].map(([mn, mx, t]) => h("label", {},
      h("input", { type: "radio", name: "len", checked: (o.min || "") === mn && (o.max || "") === mx, onchange: () => go({ min: mn, max: mx }) }), h("span", { class: "l" }, t))));
  fill(left, facet("Source", "source", SRC), facet("Kind", "kind", KIND), facet("Licence", "lic", LIC), len);
  fill(right,
    h("div", { class: "sx-card" }, h("h4", {}, "Inside long recordings"), h("p", {}, `The ten seconds where “${q || "it"}” is heard, in recordings of a minute or more.`), q ? h("a", { class: "sx-btn", href: app.href("moments", q) }, icon("clock"), "Find the moments") : null),
    h("div", { class: "sx-card" }, h("h4", {}, "As a scene"), h("p", {}, "Describe a place and get layers of sounds to play together, with the credits to paste."), q ? h("a", { class: "sx-btn", href: app.href("scene", q) }, icon("layers"), "Make it a scene") : null),
    h("div", { class: "sx-card" }, h("h4", {}, "How this ranks"), h("p", {}, "CLAP, a model of sound and text that ChloeDB runs, puts your words and every sound in one space; the search mixes that closeness with the words of each sound's title, tags and description. Steering moves the point it looks from.")));
}

// --- a sound ------------------------------------------------------------------------

async function soundPage(main, app, rest, alive) {
  const id = rest[0];
  const j = await app.api(`/v1/sound/${enc(id)}`);
  if (!alive()) return;
  const s = j.sound;
  const colors = ["--s1", "--s2", "--s3", "--s4", "--s7", "--s5"];
  const tl = j.layers.length ? h("div", {}, j.layers.map((l, i) => h("div", { class: "sx-tlrow" },
    h("span", {}, l.name),
    h("div", { class: "sx-tl" }, l.spans.map(([a, b]) => h("span", { title: `${clock(a)} to ${clock(b)}`, style: { left: `${(a / s.seconds) * 100}%`, width: `${Math.max(1.2, ((b - a) / s.seconds) * 100)}%`, background: `var(${colors[i % colors.length]})`, cursor: "pointer" }, onclick: () => app.player.play(s.id, a) }))),
    h("span", { class: "v", title: "CLAP's best odds for this label in any moment, among the 200 it listens for" }, `${Math.round(l.odds * 100)}%`))),
    h("div", { class: "sx-tlax" }, h("span", {}, "0:00"), h("span", {}, clock(s.seconds / 2)), h("span", {}, clock(s.seconds)))) : h("p", { class: "sub" }, "Nothing CLAP hears stands out among its 200 labels.");
  const credit = h("div", { class: "sx-credit" }, s.credit);
  const copy = h("button", { type: "button", class: "sx-btn", onclick: () => { navigator.clipboard?.writeText(s.credit).then(() => app.toast("Credit line copied"), () => app.toast("Copy it from the box")); } }, icon("copy"), "Copy the credit line");
  const lic = { cc0: "No conditions: use it for anything, no credit needed (a credit is still kind).", pd: "In the public domain: use it for anything.", by: "Use it for anything, also commercially, if you credit the author as above and link the licence.", "by-sa": "Use it for anything if you credit the author and share what you make from it under the same licence.", fal: "Free Art License: use and change it if you credit the author and share alike.", gfdl: "GNU Free Documentation License: credit and share alike; read its terms." }[s.lic] || "Read its licence's terms before reuse.";
  fill(main,
    h("div", { class: "sx-grid two" },
      h("div", {},
        h("div", { style: { display: "flex", gap: "14px", alignItems: "center", margin: "10px 0 6px" } }, playBtn(app, s),
          h("div", { style: { minWidth: 0 } }, h("h1", { style: { margin: 0, fontSize: "24px", letterSpacing: "-.01em", lineHeight: 1.25 } }, s.title),
            h("div", { class: "sub", style: { color: "var(--text-2)" } }, s.author ? ["by ", s.author_url ? h("a", { href: s.author_url, target: "_blank", rel: "noopener" }, s.author) : s.author, " · "] : null, SRC[s.source] || s.source, " · ", clock(s.seconds)))),
        h("div", { class: "sx-card" }, wave(app, s, { cls: "lg", n: 200 }), h("div", { class: "sx-time" }, h("span", {}, "0:00"), h("span", {}, clock(s.seconds)))),
        h("div", { class: "sx-card" }, h("h4", {}, "What is in it", h("span", { class: "sub" }, `${fmt(j.moments)} moments heard`)), tl),
        h("div", { class: "sx-sec" }, h("h3", {}, "Sounds like this"), h("a", { href: app.href("search", "", `like=${s.id}`) }, "Search from here")),
        h("div", {}, j.like.map((x) => row(app, x, { score: x.score, acts: [h("button", { type: "button", onclick: () => app.go("drift", s.id, x.id) }, "Drift between the two")] }))),
        j.more.length ? [h("div", { class: "sx-sec" }, h("h3", {}, `More by ${s.author}`)), h("div", {}, j.more.map((x) => row(app, x)))] : null),
      h("aside", {},
        h("div", { class: "sx-card" }, h("h4", {}, "Credit when you use it"), credit, h("div", { style: { display: "flex", gap: "8px", marginTop: "8px", flexWrap: "wrap" } }, copy, s.page ? h("a", { class: "sx-btn", href: s.page, target: "_blank", rel: "noopener" }, icon("link"), "Its page at the source") : null)),
        h("div", { class: "sx-card" }, h("div", { class: "sx-lic" }, h("span", { class: "cc" }, LIC[s.lic] ? LIC[s.lic].replace("CC ", "").slice(0, 5) : "CC"), h("div", {}, h("b", {}, s.licence), h("span", {}, lic), s.licence_url ? h("a", { href: s.licence_url, target: "_blank", rel: "noopener", style: { color: "var(--accent)", fontSize: "12.5px" } }, "The licence") : null))),
        h("div", { class: "sx-card sx-kv" },
          [["Source", SRC[s.source] || s.source], ["Kind", KIND[s.kind] || s.kind], ["Length", clock(s.seconds)], s.year ? ["Year", s.year] : null, s.lat != null ? ["Recorded at", `${Number(s.lat).toFixed(3)}, ${Number(s.lon).toFixed(3)}`] : null, s.genres && s.genres.length ? ["Genres", s.genres.join(", ")] : null]
            .filter(Boolean).map(([k, v]) => h("div", { class: "row" }, h("span", {}, k), h("span", {}, String(v))))),
        (s.tags && s.tags.length) ? h("div", { class: "sx-card" }, h("h4", {}, "Its tags"), h("div", { style: { display: "flex", flexWrap: "wrap", gap: "5px" } }, s.tags.slice(0, 24).map((t) => h("a", { class: "sx-chip", href: app.href("search", t) }, t)))) : null)));
}

// --- listen --------------------------------------------------------------------------

// A clip as the server reads it: one channel at 48 kHz, 16-bit WAV, at
// most ten seconds, decoded and resampled here from whatever the browser
// recorded or the file is.
async function toWav(buf, start = 0) {
  const ac = new (window.AudioContext || window.webkitAudioContext)();
  let decoded;
  try { decoded = await ac.decodeAudioData(buf.slice(0)); } finally { ac.close?.(); }
  const rate = 48000;
  const len = Math.min(decoded.duration - start, 10);
  if (len <= 0.2) throw new Error("The clip is too short.");
  const off = new OfflineAudioContext(1, Math.ceil(len * rate), rate);
  const src = off.createBufferSource();
  src.buffer = decoded;
  src.connect(off.destination);
  src.start(0, start, len);
  const out = (await off.startRendering()).getChannelData(0);
  const pcm = new DataView(new ArrayBuffer(44 + out.length * 2));
  const w = (o, s) => { for (let i = 0; i < s.length; i++) pcm.setUint8(o + i, s.charCodeAt(i)); };
  w(0, "RIFF"); pcm.setUint32(4, 36 + out.length * 2, true); w(8, "WAVE"); w(12, "fmt ");
  pcm.setUint32(16, 16, true); pcm.setUint16(20, 1, true); pcm.setUint16(22, 1, true); pcm.setUint32(24, rate, true);
  pcm.setUint32(28, rate * 2, true); pcm.setUint16(32, 2, true); pcm.setUint16(34, 16, true); w(36, "data"); pcm.setUint32(40, out.length * 2, true);
  for (let i = 0; i < out.length; i++) pcm.setInt16(44 + i * 2, Math.max(-1, Math.min(1, out[i])) * 32767, true);
  const bytes = new Uint8Array(pcm.buffer);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return { b64: btoa(bin), seconds: len, total: decoded.duration };
}

async function listen(main, app, rest, alive) {
  const out = h("div", {});
  const clockEl = h("span", { class: "clock" }, "0:00");
  const meter = h("span", {});
  const hint = h("div", { class: "hint" }, "Tap and hum, knock, whistle or play it. Ten seconds at most. The clip is heard by the model and dropped: nothing is kept.");
  const big = h("button", { type: "button", class: "big", "aria-label": "Record" }, icon("mic", ""));
  const fileIn = h("input", { type: "file", accept: "audio/*", style: { display: "none" }, onchange: () => fileIn.files[0] && useFile(fileIn.files[0]) });
  const drop = h("div", { class: "sx-drop", tabindex: "0", role: "button", onclick: () => fileIn.click(), onkeydown: (e) => { if (e.key === "Enter" || e.key === " ") fileIn.click(); } },
    h("b", {}, "Or drop a sound file here"), " (any length: the first ten seconds are heard). Find where a sound came from, who made it and its licence.");
  drop.addEventListener("dragover", (e) => e.preventDefault());
  drop.addEventListener("drop", (e) => { e.preventDefault(); e.stopPropagation(); const f = e.dataTransfer.files[0]; if (f) useFile(f); });
  fill(main,
    h("div", { class: "sx-grid two" },
      h("div", {},
        h("h1", { style: { fontSize: "26px", margin: "12px 0 4px", letterSpacing: "-.02em" } }, "Listen: search with a sound"),
        h("p", { style: { color: "var(--text-2)", margin: "0 0 14px" } }, "Hum the tune, imitate the noise, or play the clip you found. CLAP turns it into the same kind of vector as every recording here."),
        h("div", { class: "sx-rec" }, h("div", { class: "live" }, big, h("div", { style: { flex: 1 } }, h("div", { style: { display: "flex", gap: "10px", alignItems: "center" } }, clockEl, h("div", { class: "sx-meterbar" }, meter)), hint))),
        h("div", { style: { marginTop: "12px" } }, drop, fileIn),
        out),
      h("aside", {}, h("div", { class: "sx-card" }, h("h4", {}, "What happens to the clip"), h("p", {}, "Your browser cuts it to ten seconds, one channel at 48 kHz, and sends it once. ChloeDB's server embeds it with CLAP's audio tower and compares it with every recording and every moment; the clip is not stored, logged or kept.")))));
  let rec = null, stream = null, t0 = 0, timer = 0, raf = 0;
  async function start() {
    try { stream = await navigator.mediaDevices.getUserMedia({ audio: true }); } catch { app.toast("The microphone is off for this page: allow it, or drop a file."); return; }
    const chunks = [];
    rec = new MediaRecorder(stream);
    rec.ondataavailable = (e) => chunks.push(e.data);
    rec.onstop = async () => {
      stream.getTracks().forEach((t) => t.stop());
      cancelAnimationFrame(raf); clearInterval(timer);
      fill(big, icon("mic", "")); big.setAttribute("aria-label", "Record");
      const blob = new Blob(chunks, { type: rec.mimeType });
      send(await blob.arrayBuffer(), "your recording");
    };
    const ac = new AudioContext();
    const an = ac.createAnalyser();
    ac.createMediaStreamSource(stream).connect(an);
    const data = new Uint8Array(an.fftSize);
    const tick = () => { an.getByteTimeDomainData(data); let m = 0; for (const v of data) m = Math.max(m, Math.abs(v - 128)); meter.style.width = `${Math.min(100, (m / 128) * 180)}%`; raf = requestAnimationFrame(tick); };
    tick();
    rec.start();
    t0 = Date.now();
    fill(big, icon("stop", "")); big.setAttribute("aria-label", "Stop");
    timer = setInterval(() => { const s = (Date.now() - t0) / 1000; fill(clockEl, clock(s)); if (s >= 10) stop(); }, 200);
  }
  function stop() { if (rec && rec.state === "recording") rec.stop(); }
  big.addEventListener("click", () => (rec && rec.state === "recording" ? stop() : start()));
  async function useFile(f) {
    if (f.size > 200 * 1024 * 1024) { app.toast("That file is too large to read here."); return; }
    send(await f.arrayBuffer(), f.name);
  }
  async function send(buf, name) {
    fill(out, skel(3));
    let wav;
    try { wav = await toWav(buf); } catch (e) { fill(out, errBox(`Could not read ${name}: ${e.message || e}`)); return; }
    let j;
    try { j = await app.api("/v1/listen", { body: { audio: wav.b64 }, timeout: 60000 }); } catch (e) { fill(out, errBox(e)); return; }
    if (!alive()) return;
    const o = j.original;
    fill(out,
      h("div", { class: "sx-und", style: { marginTop: "16px" } }, `Heard ${wav.seconds.toFixed(1)} s of ${name}${wav.total > wav.seconds + 0.5 ? ` (of ${clock(wav.total)})` : ""}.`),
      o ? h("div", { class: "sx-orig" }, h("h4", {}, "Probably cut from"), row(app, o, { big: true, score: o.score, moments: [{ t: o.t, score: o.score }], bands: [[o.t, o.t + 10]] }), h("div", { class: "sub" }, "Its credit: ", o.credit)) : null,
      h("div", { class: "sx-sec" }, h("h3", {}, "Sounds like it")),
      h("div", {}, j.like.map((s) => row(app, s, { score: s.score }))),
      j.moments.length ? [h("div", { class: "sx-sec" }, h("h3", {}, "Moments like it, inside recordings")), h("div", {}, j.moments.map((m) => row(app, m, { score: m.score, moments: m.at, bands: m.at.map((a) => [a.t, a.t + 10]) })))] : null);
  }
  if (rest[0] === "file" && app.pendingFile) { const f = app.pendingFile; app.pendingFile = null; useFile(f); }
}

// --- moments ---------------------------------------------------------------------------

async function moments(main, app, rest, alive) {
  const q = rest[0] || "";
  const input = h("input", { type: "search", value: q, placeholder: "What to listen for: thunder, a church bell, applause…", "aria-label": "What to listen for", style: { font: "15px var(--sans)", padding: "9px 12px", borderRadius: "10px", border: "1px solid var(--line-2)", background: "var(--surface)", color: "var(--text)", flex: 1, minWidth: "220px" }, onkeydown: (e) => { if (e.key === "Enter" && input.value.trim()) app.go("moments", input.value.trim()); } });
  const body = h("div", {}, q ? skel(4) : null);
  fill(main,
    h("h1", { style: { fontSize: "26px", margin: "12px 0 4px", letterSpacing: "-.02em" } }, "Moments: the seconds you need inside long recordings"),
    h("p", { style: { color: "var(--text-2)", margin: "0 0 12px", maxWidth: "820px" } }, "Every recording is heard ten seconds at a time, every five seconds. A search finds the moments, best recording first, and plays from there."),
    h("div", { class: "sx-pick" }, input, h("button", { type: "button", class: "sx-btn pri", onclick: () => input.value.trim() && app.go("moments", input.value.trim()) }, "Listen for it")),
    h("div", { class: "sx-und", style: { marginTop: "10px" } }, "Try:", ["thunder", "church bells", "a dog barking", "applause", "a train horn", "birdsong", "laughter"].map((x) => h("a", { class: "sx-chip", href: app.href("moments", x) }, x))),
    body);
  if (!q) return;
  const j = await app.api(`/v1/moments?q=${enc(q)}`);
  if (!alive()) return;
  fill(body, j.results.length ? j.results.map((s) => row(app, s, { big: true, score: s.score, moments: s.at, bands: s.at.map((a) => [a.t, a.t + 10]) })) : h("div", { class: "sx-empty" }, "No long recording has that."));
}

// --- scenes ------------------------------------------------------------------------------

async function scene(main, app, rest, alive) {
  const q = rest[0] || "";
  const input = h("input", { type: "search", value: q, placeholder: "Describe a place: a harbour at dawn, a rainy café, a farm in summer", "aria-label": "Describe a place", style: { font: "15px var(--sans)", padding: "9px 12px", borderRadius: "10px", border: "1px solid var(--line-2)", background: "var(--surface)", color: "var(--text)", flex: 1, minWidth: "220px" }, onkeydown: (e) => { if (e.key === "Enter" && input.value.trim()) app.go("scene", input.value.trim()); } });
  const body = h("div", {}, q ? skel(4) : null);
  fill(main,
    h("h1", { style: { fontSize: "26px", margin: "12px 0 4px", letterSpacing: "-.02em" } }, "Scenes: describe a place, hear it in layers"),
    h("p", { style: { color: "var(--text-2)", margin: "0 0 12px", maxWidth: "820px" } }, "The words CLAP hears closest to your place become layers; each layer gets the sound that best fits both. Play them together, set each one's level, and take the credits with you."),
    h("div", { class: "sx-pick" }, input, h("button", { type: "button", class: "sx-btn pri", onclick: () => input.value.trim() && app.go("scene", input.value.trim()) }, "Make the scene")),
    h("div", { class: "sx-und", style: { marginTop: "10px" } }, "Try:", ["a harbour at dawn", "a rainy café", "a farm in summer", "a thunderstorm in the city", "a night in the jungle", "a kitchen at dinner time"].map((x) => h("a", { class: "sx-chip", href: app.href("scene", x) }, x))),
    body);
  if (!q) return;
  const j = await app.api(`/v1/scene?q=${enc(q)}`);
  if (!alive()) return;
  // The layers play together, each its own element, looped.
  const els = [];
  const mixBtn = h("button", { type: "button", class: "sx-btn pri" }, icon("play"), "Play the scene");
  let playing = false;
  const stopAll = () => { for (const e of els) e.a.pause(); playing = false; fill(mixBtn, icon("play"), "Play the scene"); };
  mixBtn.addEventListener("click", async () => {
    if (playing) return stopAll();
    app.player.stop();
    playing = true;
    fill(mixBtn, icon("stop"), "Stop");
    for (const e of els) {
      if (!e.a.src) e.a.src = await audioSrc(app, e.s.id);
      e.a.loop = true;
      e.a.volume = e.mute ? 0 : e.gain;
      e.a.currentTime = 0;
      e.a.play().catch(() => {});
    }
  });
  const layers = j.layers.map((l) => {
    const e = { s: l.sound, gain: l.gain, mute: false, a: new Audio() };
    els.push(e);
    const vol = h("input", { type: "range", min: 0, max: 1, step: 0.05, value: l.gain, "aria-label": `Level of ${l.label}`, oninput: () => { e.gain = Number(vol.value); e.a.volume = e.mute ? 0 : e.gain; } });
    const mute = h("button", { type: "button", class: "sx-btn", onclick: () => { e.mute = !e.mute; e.a.volume = e.mute ? 0 : e.gain; mute.classList.toggle("on", e.mute); } }, "Mute");
    return h("div", { class: "sx-layer" },
      h("div", { class: "nm" }, l.label, h("small", {}, l.group)),
      h("div", { class: "pick" }, h("a", { class: "t", href: app.href("sound", l.sound.id) }, l.sound.title), h("div", { class: "b" }, badge(l.sound), l.sound.author, " · ", clock(l.sound.seconds)), wave(app, l.sound, { cls: "sm", n: 80 })),
      vol, h("div", { class: "sw" }, mute));
  });
  const credits = j.credits.join("\n");
  fill(body,
    h("div", { class: "sx-card" }, h("div", { class: "sx-mix" }, mixBtn, h("span", { class: "sub" }, `${j.layers.length} layers, looped together`)), layers.length ? layers : h("div", { class: "sx-empty" }, "No layers for that place.")),
    h("div", { class: "sx-card" }, h("h4", {}, "Credits to paste"), h("div", { class: "sx-credit" }, credits),
      h("button", { type: "button", class: "sx-btn", style: { marginTop: "8px" }, onclick: () => navigator.clipboard?.writeText(credits).then(() => app.toast("Credits copied")) }, icon("copy"), "Copy the credits")));
  const off = () => { if (!main.isConnected || !alive()) { for (const e of els) { e.a.pause(); e.a.src = ""; } clearInterval(iv); } };
  const iv = setInterval(off, 500);
}

async function audioSrc(app, id) {
  if (app.ctx.media) return app.ctx.media(`/v1/audio/${enc(id)}`);
  const j = await app.ctx.api(`/v1/audio/${enc(id)}?b64=1`, { timeout: 60000 });
  const bin = atob(j.data);
  const a = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
  return URL.createObjectURL(new Blob([a], { type: j.mime }));
}

// --- map ----------------------------------------------------------------------------------

async function map(main, app, rest, alive) {
  const by = rest[0] === "place" ? "place" : "sound";
  const wrap = h("div", { class: "sx-map" });
  const canvas = h("canvas", { class: "sx-canvas", "aria-label": by === "place" ? "Every sound with a place, on a map of the world" : "Every sound, placed by how it sounds: near ones sound alike" });
  const tip = h("div", { class: "sx-tip" });
  const card = h("div", {});
  wrap.append(canvas, tip);
  const legend = h("div", { class: "sx-legend" }, Object.entries(KIND).map(([k, t]) => h("span", {}, h("i", { style: { background: `var(${KCOLOR[k]})` } }), t)));
  fill(main,
    h("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "end", gap: "12px", flexWrap: "wrap", margin: "12px 0 6px" } },
      h("div", {}, h("h1", { style: { fontSize: "26px", margin: 0, letterSpacing: "-.02em" } }, by === "place" ? "Map: where the sounds were recorded" : "Map: every sound by how it sounds"),
        h("p", { style: { color: "var(--text-2)", margin: "4px 0 0" } }, by === "place" ? "The recordings that say where they were made. Click one to hear it." : "Sounds that sound alike sit together (a t-SNE layout of their CLAP vectors). Drag to move, scroll to zoom, click to hear.")),
      h("div", { class: "sx-seg" }, h("button", { type: "button", class: by === "sound" ? "on" : "", onclick: () => app.go("map") }, "By how it sounds"), h("button", { type: "button", class: by === "place" ? "on" : "", onclick: () => app.go("map", "place") }, "By where recorded"))),
    legend, wrap, card);
  const j = await app.api("/v1/map");
  if (!alive()) return;
  let pts = j.points.map(([id, title, kind, source, x, y, lat, lon, seconds]) => ({ id, title, kind, source, x, y, lat, lon, seconds }));
  if (by === "place") pts = pts.filter((p) => p.lat != null && p.lon != null).map((p) => ({ ...p, x: (p.lon + 180) / 360, y: (90 - p.lat) / 180 }));
  let scale = 1, ox = 0, oy = 0, hover = null, picked = null;
  const ctx2 = canvas.getContext("2d");
  function size() { const r = canvas.getBoundingClientRect(); canvas.width = r.width * devicePixelRatio; canvas.height = r.height * devicePixelRatio; }
  const pos = (p) => {
    const W = canvas.width, H = canvas.height, pad = 30 * devicePixelRatio;
    const aspect = by === "place" ? Math.min((W - 2 * pad) / 2, H - 2 * pad) : 0;
    const w = by === "place" ? aspect * 2 : W - 2 * pad, hh = by === "place" ? aspect : H - 2 * pad;
    const x0 = (W - w) / 2, y0 = (H - hh) / 2;
    return [x0 + (p.x * w - w / 2) * scale + w / 2 + ox, y0 + (p.y * hh - hh / 2) * scale + hh / 2 + oy];
  };
  function draw() {
    const W = canvas.width, H = canvas.height;
    ctx2.clearRect(0, 0, W, H);
    if (by === "place") {
      ctx2.strokeStyle = cssVar("--line", canvas); ctx2.lineWidth = 1;
      for (let lon = -180; lon <= 180; lon += 30) { const [a, b] = pos({ x: (lon + 180) / 360, y: 0 }); const [, d] = pos({ x: (lon + 180) / 360, y: 1 }); ctx2.beginPath(); ctx2.moveTo(a, b); ctx2.lineTo(a, d); ctx2.stroke(); }
      for (let lat = -90; lat <= 90; lat += 30) { const [a, b] = pos({ x: 0, y: (90 - lat) / 180 }); const [c] = pos({ x: 1, y: (90 - lat) / 180 }); ctx2.beginPath(); ctx2.moveTo(a, b); ctx2.lineTo(c, b); ctx2.stroke(); }
    }
    const r = (by === "place" ? 4 : 3) * devicePixelRatio * Math.min(2.2, Math.sqrt(scale));
    const cols = {};
    for (const k of Object.keys(KIND)) cols[k] = cssVar(KCOLOR[k], canvas);
    ctx2.globalAlpha = 0.8;
    for (const p of pts) { const [x, y] = pos(p); if (x < -10 || y < -10 || x > W + 10 || y > H + 10) continue; ctx2.fillStyle = cols[p.kind] || "#888"; ctx2.beginPath(); ctx2.arc(x, y, r, 0, 7); ctx2.fill(); }
    ctx2.globalAlpha = 1;
    for (const p of [hover, picked].filter(Boolean)) { const [x, y] = pos(p); ctx2.strokeStyle = cssVar("--text", canvas); ctx2.lineWidth = 2 * devicePixelRatio; ctx2.beginPath(); ctx2.arc(x, y, r * 2, 0, 7); ctx2.stroke(); }
  }
  function nearest(e) {
    const rc = canvas.getBoundingClientRect();
    const mx = (e.clientX - rc.left) * devicePixelRatio, my = (e.clientY - rc.top) * devicePixelRatio;
    let best = null, bd = (14 * devicePixelRatio) ** 2;
    for (const p of pts) { const [x, y] = pos(p); const d = (x - mx) ** 2 + (y - my) ** 2; if (d < bd) { bd = d; best = p; } }
    return best;
  }
  let drag = null, moved = false;
  canvas.addEventListener("pointerdown", (e) => { drag = [e.clientX, e.clientY, ox, oy]; moved = false; canvas.setPointerCapture(e.pointerId); });
  canvas.addEventListener("pointermove", (e) => {
    if (drag) { const dx = e.clientX - drag[0], dy = e.clientY - drag[1]; if (Math.abs(dx) + Math.abs(dy) > 3) moved = true; ox = drag[2] + dx * devicePixelRatio; oy = drag[3] + dy * devicePixelRatio; draw(); return; }
    const p = nearest(e);
    if (p !== hover) { hover = p; draw(); }
    if (p) { const rc = wrap.getBoundingClientRect(); tip.style.display = "block"; tip.style.left = `${Math.min(e.clientX - rc.left + 12, rc.width - 290)}px`; tip.style.top = `${e.clientY - rc.top + 12}px`; fill(tip, h("b", {}, p.title), h("div", { class: "sub" }, `${KIND[p.kind] || p.kind} · ${SRC[p.source] || p.source} · ${clock(p.seconds)}`)); } else tip.style.display = "none";
  });
  canvas.addEventListener("pointerup", (e) => {
    const wasDrag = moved; drag = null;
    if (wasDrag) return;
    const p = nearest(e);
    if (!p) return;
    picked = p; draw();
    app.player.play(p.id);
    app.api(`/v1/sound/${enc(p.id)}`).then((s) => { if (alive()) fill(card, h("div", { class: "sx-card", style: { marginTop: "12px" } }, row(app, s.sound, { big: true }))); }).catch(() => {});
  });
  canvas.addEventListener("pointerleave", () => { tip.style.display = "none"; hover = null; draw(); });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const rc = canvas.getBoundingClientRect();
    const mx = (e.clientX - rc.left) * devicePixelRatio - canvas.width / 2, my = (e.clientY - rc.top) * devicePixelRatio - canvas.height / 2;
    const f = Math.exp(-e.deltaY * 0.0015);
    const ns = Math.min(40, Math.max(0.6, scale * f));
    const k = ns / scale;
    ox = mx - (mx - ox) * k; oy = my - (my - oy) * k; scale = ns;
    draw();
  }, { passive: false });
  size(); draw();
  const ro = new ResizeObserver(() => { size(); draw(); });
  ro.observe(canvas);
}

// --- music and drifts ------------------------------------------------------------------

async function music(main, app, rest, alive) {
  const body = h("div", {}, skel(4));
  fill(main,
    h("h1", { style: { fontSize: "26px", margin: "12px 0 4px", letterSpacing: "-.02em" } }, "Music"),
    h("p", { style: { color: "var(--text-2)", margin: "0 0 12px", maxWidth: "820px" } }, "Songs of the Free Music Archive whose licence allows any use with credit. Search them by how they sound, or drift from one song to another: each step is the song nearest the point that far along the way."),
    h("div", { class: "sx-und" }, "Try:", ["a slow piano melody", "upbeat electronic dance", "acoustic folk with a singer", "dark ambient drone", "jazz with a saxophone", "lo-fi hip hop beat"].map((x) => h("a", { class: "sx-chip", href: app.href("search", x, "kind=music") }, x))),
    body);
  const j = await app.api(`/v1/search?kind=music&per=24`);
  if (!alive()) return;
  const [a, b] = [j.results[0], j.results[Math.min(j.results.length - 1, 7)]];
  fill(body,
    a && b ? h("div", { class: "sx-card" }, h("h4", {}, "Drift between two songs"), h("p", {}, `From “${a.title}” to “${b.title}”, or pick any two from a song's page (Sounds like this → Drift between the two).`), h("a", { class: "sx-btn pri", href: app.href("drift", a.id, b.id) }, icon("drift"), "Drift")) : null,
    h("div", { class: "sx-sec" }, h("h3", {}, "Songs"), h("a", { href: app.href("search", "", "kind=music") }, "Search all")),
    j.results.map((s) => row(app, s, { acts: [h("button", { type: "button", onclick: () => app.go("search", "", `kind=music&like=${s.id}`) }, "More like this")] })));
}

async function drift(main, app, rest, alive) {
  const [from, to] = rest;
  const j = await app.api(`/v1/drift?from=${enc(from)}&to=${enc(to)}&steps=8`);
  if (!alive()) return;
  const path = j.path;
  const steps = h("div", { class: "sx-drift" });
  let at = -1, on = false;
  const playBtnAll = h("button", { type: "button", class: "sx-btn pri" }, icon("play"), "Play the drift");
  const drawSteps = () => fill(steps, path.map((s, i) => h("div", { class: `sx-step${i === 0 || i === path.length - 1 ? " end" : ""}${i === at ? " on" : ""}`, onclick: () => { at = i; on = true; next(true); } },
    h("span", { class: "k" }, i === 0 ? "FROM" : i === path.length - 1 ? "TO" : `STEP ${i}`), h("b", {}, s.title), h("span", {}, `${s.author || ""} · ${clock(s.seconds)}`))));
  // Each step plays for up to 12 seconds, then the next.
  let timer = 0;
  function next(here) {
    clearTimeout(timer);
    if (!here) at++;
    if (at >= path.length || !main.isConnected || !alive()) { on = false; at = -1; drawSteps(); fill(playBtnAll, icon("play"), "Play the drift"); return; }
    drawSteps();
    app.player.play(path[at].id, 0);
    timer = setTimeout(() => next(false), Math.min(12, path[at].seconds || 12) * 1000);
  }
  playBtnAll.addEventListener("click", () => { if (on) { on = false; clearTimeout(timer); app.player.stop(); at = -1; drawSteps(); fill(playBtnAll, icon("play"), "Play the drift"); return; } on = true; at = -1; fill(playBtnAll, icon("stop"), "Stop"); next(false); });
  drawSteps();
  fill(main,
    h("h1", { style: { fontSize: "26px", margin: "12px 0 4px", letterSpacing: "-.02em" } }, "Drift"),
    h("p", { style: { color: "var(--text-2)", margin: "0 0 12px" } }, `${path.length} sounds from “${path[0]?.title}” to “${path[path.length - 1]?.title}”: each step is the sound nearest the point that far along the way between the two, in CLAP's space.`),
    h("div", { class: "sx-mix" }, playBtnAll, h("span", { class: "sub" }, "about 12 seconds of each")),
    steps,
    h("div", { class: "sx-sec" }, h("h3", {}, "The path, with credits")),
    path.map((s) => row(app, s)));
}

// --- how it works ----------------------------------------------------------------------------

async function about(main, app, _rest, alive) {
  const j = await app.api("/v1/status");
  if (!alive()) return;
  const st = j.stats;
  const size = (t) => (t.bytes != null ? `${(t.bytes / 1048576).toFixed(1)} MB` : t.disk_bytes != null ? `${(t.disk_bytes / 1048576).toFixed(1)} MB` : "");
  const rowsOf = (name) => (j.tables || []).find((t) => t.table === name) || {};
  fill(main,
    h("div", { style: { margin: "14px 0 4px" } },
      h("h1", { style: { margin: 0, fontSize: "28px", letterSpacing: "-.02em" } }, "How it works"),
      h("div", { style: { color: "var(--text-2)", fontSize: "15px", marginTop: "4px", maxWidth: "860px" } }, "Sound Explorer is one ChloeDB database. ChloeDB's server runs CLAP, a model of sound and text, so the sounds, their moments, the words of a search and the clip you hum all become vectors in one space, and every page is SQL over three tables. The SQL button on every page shows the queries it ran.")),
    h("div", { class: "sx-kpis" },
      h("div", { class: "sx-kpi" }, h("b", {}, fmt(st.sounds)), h("span", {}, "recordings and tracks"), h("small", {}, `${fmt(st.hours)} hours of sound`)),
      h("div", { class: "sx-kpi" }, h("b", {}, fmt(st.moments)), h("span", {}, "moments, each heard on its own"), h("small", {}, "10 seconds, every 5 seconds")),
      h("div", { class: "sx-kpi" }, h("b", {}, fmt(st.labels)), h("span", {}, "labels CLAP listens for"), h("small", {}, "FSD50K's classes, for “what is in it”")),
      h("div", { class: "sx-kpi" }, h("b", {}, `${Math.round(app.took)} ms`), h("span", {}, "this page's queries"), h("small", {}, "open the SQL button for each one"))),
    h("div", { class: "sx-sec" }, h("h3", {}, "Where the sounds come from")),
    h("div", { class: "sx-card sx-tbl-wrap" }, h("table", { class: "sx-tbl" },
      h("thead", {}, h("tr", {}, ["Source", "What", "Here", "Licences kept", "Link back"].map((x) => h("th", {}, x)))),
      h("tbody", {}, [
        ["fsd50k", "Freesound clips of 200 kinds, released by Freesound's team for reuse (FSD50K)", "CC0 and CC BY clips only", "Each clip's Freesound page and author"],
        ["commons", "Field recordings, animals, music, machines and weather from Wikimedia Commons", "Free licences (CC0, CC BY, CC BY-SA, public domain)", "Each file's Commons page"],
        ["fma", "Songs from the Free Music Archive dataset (EPFL)", "Only tracks that allow commercial use and excerpts: CC BY, CC BY-SA, CC0, Free Art", "Each track's FMA page and artist"],
      ].map(([k, what, lic, link]) => { const s = st.sources.find((x) => x.source === k) || {}; return h("tr", {}, h("td", {}, h("b", {}, SRC[k])), h("td", {}, what), h("td", { class: "n" }, `${fmt(s.n || 0)} · ${fmt(s.hours || 0)} h`), h("td", {}, lic), h("td", {}, link)); })))),
    h("div", { class: "sx-sec" }, h("h3", {}, "From a recording to a search")),
    h("div", { class: "sx-flow" },
      h("div", { class: "st" }, h("span", { class: "k" }, "1 · FETCH"), h("b", {}, "Files and their facts"), "Title, words, tags, author, licence and place, from each source's own pages, at its pace."),
      h("div", { class: "st" }, h("span", { class: "k" }, "2 · LISTEN"), h("b", {}, "CLAP inside ChloeDB"), "Each recording is cut into ten-second windows every five seconds; ChloeDB's server turns each into 512 numbers (clap-htsat-unfused)."),
      h("div", { class: "st" }, h("span", { class: "k" }, "3 · KEEP"), h("b", {}, "Three tables"), "A row a recording with its words and the mean of its windows, a row a moment, and the labels."),
      h("div", { class: "st" }, h("span", { class: "k" }, "4 · SEARCH"), h("b", {}, "Words and sound, one query"), "BM25 on the words and similarity on the vectors, fused in SQL; your words or your clip become a vector the same way."),
      h("div", { class: "st" }, h("span", { class: "k" }, "5 · SERVE"), h("b", {}, "Read-only pages"), "Rate limited, no public SQL. A clip you record is the only thing sent: ten seconds at most, heard and dropped.")),
    h("div", { class: "sx-cols two", style: { marginTop: "16px" } },
      h("div", { class: "sx-card" }, h("h4", {}, "The tables, now"),
        h("table", { class: "sx-tbl" }, h("thead", {}, h("tr", {}, ["Table", "One row a", "Rows", "Size"].map((x) => h("th", {}, x)))),
          h("tbody", {}, [["au_sounds", "recording or track", st.sounds], ["au_moments", "ten seconds of one", st.moments], ["au_labels", "word CLAP listens for", st.labels]].map(([t, what, n]) => h("tr", {}, h("td", {}, h("code", {}, t)), h("td", {}, what), h("td", { class: "n" }, fmt(n)), h("td", { class: "n" }, size(rowsOf(t)))))))),
      h("div", { class: "sx-card" }, h("h4", {}, "Licences of what is here"), h("div", { class: "sx-bars" }, st.licences.map((l) => [h("span", { class: "t" }, LIC[l.lic] || l.lic), h("div", { class: "m" }, h("span", { style: { width: `${(l.n / Math.max(1, st.sounds)) * 100}%` } })), h("span", { class: "v" }, fmt(l.n))])))),
    h("div", { class: "sx-card" }, h("h4", {}, "The tables, as created"), h("pre", { class: "sx-credit", style: { whiteSpace: "pre-wrap" } }, `CREATE TABLE au_sounds (
  id KEYWORD PRIMARY KEY, title TEXT, words TEXT,
  author KEYWORD, source KEYWORD, licence KEYWORD, lic KEYWORD, kind KEYWORD,
  tags KEYWORD[], labels KEYWORD[], genres KEYWORD[],
  seconds FLOAT, year INT, lat FLOAT, lon FLOAT, x FLOAT, y FLOAT, ...,
  emb VECTOR(512)          -- the mean of its moments' CLAP vectors
);
CREATE TABLE au_moments (
  id KEYWORD PRIMARY KEY, sound KEYWORD NOT NULL, t FLOAT, seconds FLOAT,
  emb VECTOR(512)          -- ten seconds from t, by CLAP
) OPTIONS(shard_by = 'sound', cluster_by = 'sound');
CREATE TABLE au_labels (
  id KEYWORD PRIMARY KEY, name TEXT, grp KEYWORD, prompt TEXT,
  emb VECTOR(512) OPTIONS(embed = 'prompt', model = 'clap-htsat-unfused')
);`)));
}

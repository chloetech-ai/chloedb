// Earth on ChloeDB: California and Chile from space. The page, in ChloeDB's
// command center (#/app/earth, loaded through chloedb-server from this app)
// and on its own (the app's / when it has no token, through ui/index.html).
// It builds its own elements, so the same file serves both, and loads one
// library, MapLibre GL, from jsDelivr at a pinned version.
//
//   (home)                 what it is, both regions now, the replays
//   live/<region>          the fires GOES sees burning now, and its last hour
//   replay/<id>            a past day played back, scan by scan
//   sky/<region>           Clear sky: the clouds now, the clear nights and
//                          days of 30, and a finder of clear places
//   watch[/<id>]           Fire watch: places a browser keeps an eye on,
//                          each checked on every scan, and its alerts;
//                          watch/example plays the Park Fire's first hours
//   fire/<id>              a fire: its pixels, its first hours, the fires
//                          that began like it and what they became
//   fires/<region>         the past fires, the largest first
//   explore/<region>       the map with a layer (trees, people, climate...)
//   place/<id>             a place: what lies on it, the places that look
//                          like it, its climate twins, mixing it with words
//   search/<q>             places by what they look like, in words
//   picture                places that look like a picture
//   twins[/<from>]         Climate twins: California and Chile side by side
//   changed[/<q>]          Changed places: what places became
//   rules, data            the alert rules, and how it is built
//
// The map's picture of the ground is EOX's Sentinel-2 cloudless 2016 (CC BY
// 4.0); a place's thumbnail is the same picture of its square, from EOX's
// WMS. Every answer of the API carries the SQL it ran: "SQL" shows it.

export default { render };

const MAPLIBRE = "https://cdn.jsdelivr.net/npm/maplibre-gl@4.7.1/dist/maplibre-gl";
const EOX = "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/{z}/{y}/{x}.jpg";
const LABELS = "https://tiles.maps.eox.at/wmts/1.0.0/overlay_bright_3857/default/g/{z}/{y}/{x}.png";
const DEM = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png";
const ATTRIB = '<a href="https://s2maps.eu" target="_blank" rel="noopener">Sentinel-2 cloudless 2016</a> by EOX IT Services GmbH (contains modified Copernicus Sentinel data 2016), labels © OpenStreetMap contributors, terrain: Mapzen terrain tiles';
// The picture of the sky now: GOES's GeoColor from NASA's GIBS, every 10
// minutes, about half an hour after the scan. GOES-West sees California and
// GOES-East sees Chile. GIBS lists the times it has; a time just listed can
// still lack tiles for a minute, so the newest one whose tiles answer (one
// far out and one close in, over the region) is taken, and the tiles are
// asked for at that time, so every tile on the map is the same picture.
const GIBS = "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best";
const SKY = { california: { layer: "GOES-West_ABI_GeoColor", probes: ["4/6/2", "7/49/21"] }, chile: { layer: "GOES-East_ABI_GeoColor", probes: ["4/9/4", "7/77/38"] } };
const SKY_ATTRIB = 'Satellite now: GOES GeoColor (NOAA, CIRA) from <a href="https://www.earthdata.nasa.gov/gibs" target="_blank" rel="noopener">NASA GIBS</a>';
const skyTiles = (layer, t) => `${GIBS}/${layer}/default/${t}/GoogleMapsCompatible_Level7/{z}/{y}/{x}.png`;
const isoMin = (ms) => new Date(ms).toISOString().replace(/\.\d+Z$/, "Z");
async function skyTime(region) {
  const k = SKY[region];
  const r = await fetch(`${GIBS}/1.0.0/${k.layer}/default/GoogleMapsCompatible_Level7/all/${isoMin(Date.now() - 6 * 3600e3)}--${isoMin(Date.now() + 3600e3)}.xml`, { cache: "no-store" });
  if (!r.ok) throw new Error(`GIBS answered ${r.status}`);
  const dom = (await r.text()).match(/<Domain>([^<]*)<\/Domain>/)?.[1] || "";
  const times = [];
  for (const part of dom.split(",").reverse()) {
    const [a, b] = part.split("/").map(Date.parse);
    for (let t = b; t >= a && times.length < 6; t -= 600e3) times.push(t);
  }
  for (const t of times) {
    const at = isoMin(t);
    const ok = await Promise.all(k.probes.map((z) => fetch(skyTiles(k.layer, at).replace("{z}/{y}/{x}", z), { method: "HEAD" }).then((x) => x.ok, () => false)));
    if (ok.every(Boolean)) return at;
  }
  throw new Error("no recent picture");
}
const HALF = 3.84 / 2 / 111.32;
// Half a GOES fire pixel, in degrees of latitude (about 2.5 km a side over
// California from GOES-West and over central Chile from GOES-East).

const REGION = {
  california: { title: "California", tz: "America/Los_Angeles", center: [-119.4, 37.2], zoom: 5.2, bounds: [[-124.6, 32.4], [-114.0, 42.1]], sat: "GOES-18", other: "chile" },
  chile: { title: "Chile", tz: "America/Santiago", center: [-71.3, -35.5], zoom: 4.3, bounds: [[-76.0, -56.2], [-66.3, -17.4]], sat: "GOES-19", other: "california" },
};

const KOPPEN = { Af: "#0000ff", Am: "#0078ff", Aw: "#46aafa", BWh: "#ff0000", BWk: "#ff9696", BSh: "#f5a500", BSk: "#ffdc64", Csa: "#ffff00", Csb: "#c8c800", Csc: "#969600", Cwa: "#96ff96", Cwb: "#64c864", Cwc: "#329632", Cfa: "#c8ff50", Cfb: "#64ff50", Cfc: "#32c800", Dsa: "#ff00ff", Dsb: "#c800c8", Dsc: "#963296", Dsd: "#966496", Dwa: "#aaafff", Dwb: "#5a78dc", Dwc: "#4b50b4", Dwd: "#320087", Dfa: "#00ffff", Dfb: "#37c8ff", Dfc: "#007d7d", Dfd: "#00465f", ET: "#b2b2b2", EF: "#666666" };
const KOPPEN_WORDS = { Csa: "Mediterranean, hot summer", Csb: "Mediterranean, warm summer", Csc: "Mediterranean, cool summer", BWh: "hot desert", BWk: "cold desert", BSh: "hot steppe", BSk: "cold steppe", Cfb: "oceanic", Cfc: "subpolar oceanic", Cfa: "humid subtropical", ET: "tundra", EF: "ice cap", Dsb: "cold, dry summer", Dsc: "cold, dry summer", Dfb: "cold, wet", Dfc: "subarctic", Dsa: "cold, hot dry summer", Dfa: "cold, hot summer" };
const KIND = { forest: "#1f7a1f", shrubland: "#d9a21b", grassland: "#d8d84a", farmland: "#d77ee0", city: "#e03b3b", bare: "#a8a29a", "snow and ice": "#e8eef4", water: "#2a78d6", wetland: "#119fa6", tundra: "#c9b98a", mine: "#6b4a2b", unknown: "#777" };
const COVER = [["trees", "Trees", "#1f7a1f"], ["shrubs", "Shrubs", "#d9a21b"], ["grass", "Grass", "#d8d84a"], ["crops", "Farms", "#d77ee0"], ["built", "Built", "#e03b3b"], ["bare", "Bare", "#a8a29a"], ["water", "Water", "#2a78d6"], ["snow", "Snow", "#c9d6e3"]];
const LAYER_LIST = [["", "Picture only"], ["fires", "Fire history"], ["trees", "Trees"], ["shrubs", "Shrubs"], ["crops", "Farms"], ["built", "Built"], ["people", "People"], ["elev", "Terrain"], ["slope", "Slope"], ["climate", "Climate"], ["kind", "What it is"], ["mine", "Mines"], ["water", "Water"], ["snow", "Snow and ice"], ["crop", "Crops (California)"]];
const RAMP = { fires: ["#fff3e0", "#ffb74d", "#f57c00", "#c62828", "#6d0f0f"], people: ["#f3e5f5", "#ce93d8", "#8e24aa", "#4a148c", "#1a0033"], elev: ["#e8f5e9", "#a5d6a7", "#c5b358", "#8d6e63", "#f5f5f5"], default: ["#e3f2e3", "#a5d6a7", "#4caf50", "#1b5e20", "#08300a"] };

const CSS = `
.ea { --fire: #eb6834; --fire-ink: #b8451b; }
.ea-head { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 8px; }
.ea-head h1 { margin: 0; font-size: 20px; letter-spacing: -.01em; display: flex; align-items: center; gap: 8px; }
.ea-head h1 svg { width: 22px; height: 22px; stroke: var(--fire); fill: none; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.ea-head .sub { color: var(--muted); font-size: 12.5px; }
.ea-head .tools { margin-left: auto; display: flex; gap: 8px; align-items: center; }
.ea-tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); margin-bottom: 14px; overflow-x: auto; scrollbar-width: none; }
.ea-tabs a { padding: 8px 13px; color: var(--muted); font-weight: 500; border-bottom: 2px solid transparent; white-space: nowrap; text-decoration: none; }
.ea-tabs a:hover { color: var(--text); text-decoration: none; }
.ea-tabs a.on { color: var(--text); border-bottom-color: var(--accent); }
.ea-tabs .n { font-size: 11px; padding: 0 6px; border-radius: 9px; background: var(--crit-soft); color: var(--crit-ink); margin-left: 4px; }
.ea-split { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 400px); gap: 14px; align-items: start; }
.ea-split.wide { grid-template-columns: minmax(0, 1fr) minmax(320px, 460px); }
@media (max-width: 900px) { .ea-split, .ea-split.wide { grid-template-columns: minmax(0, 1fr); } }
.ea-map { position: relative; height: min(72vh, 720px); min-height: 360px; border-radius: var(--r); overflow: hidden; border: 1px solid var(--line); background: #0d1b12; }
.ea-map.short { height: min(52vh, 520px); }
.ea-map .maplibregl-ctrl-attrib { font-size: 10px; }
.ea-maptl { position: absolute; left: 10px; top: 10px; z-index: 2; display: flex; flex-direction: column; gap: 6px; align-items: flex-start; pointer-events: none; }
.ea-maptl > * { pointer-events: auto; }
/* On a phone the title goes under the buttons rather than behind them. */
@media (max-width: 640px) { .ea-maptl { top: 46px; right: 10px; } }
.ea-mapttl { background: rgba(13, 15, 18, .72); color: #fff; padding: 5px 10px; border-radius: 7px; font-size: 13px; }
.ea-onair { display: inline-flex; align-items: center; gap: 7px; background: rgba(13, 15, 18, .72); color: #fff; padding: 4px 10px 4px 8px; border-radius: 999px; font-size: 12px; }
.ea-onair b { letter-spacing: .06em; font-size: 11px; }
.ea-onair .light { width: 10px; height: 10px; border-radius: 50%; background: #ff3b30; box-shadow: 0 0 0 0 rgba(255, 59, 48, .7); animation: ea-pulse 1.6s infinite; }
.ea-onair.off .light { background: #9aa3ae; animation: none; box-shadow: none; }
.ea-onair .t { opacity: .8; }
@keyframes ea-pulse { 0% { box-shadow: 0 0 0 0 rgba(255, 59, 48, .7); } 70% { box-shadow: 0 0 0 9px rgba(255, 59, 48, 0); } 100% { box-shadow: 0 0 0 0 rgba(255, 59, 48, 0); } }
@media (prefers-reduced-motion: reduce) { .ea-onair .light { animation: none; } }
.ea-maptr { position: absolute; right: 10px; top: 10px; z-index: 2; display: flex; gap: 6px; flex-wrap: wrap; justify-content: flex-end; }
.ea-mapbtn { background: rgba(13, 15, 18, .72); color: #fff; border: 0; border-radius: 7px; padding: 5px 10px; font-size: 12.5px; cursor: pointer; }
.ea-mapbtn.on { background: var(--accent); }
.ea-legend { position: absolute; left: 10px; bottom: 26px; z-index: 2; background: rgba(13, 15, 18, .78); color: #fff; padding: 8px 10px; border-radius: 8px; font-size: 11.5px; max-width: 280px; }
.ea-legend .bar { height: 8px; border-radius: 4px; margin: 4px 0 2px; }
.ea-legend .ends { display: flex; justify-content: space-between; opacity: .85; }
.ea-legend .sw { display: inline-flex; align-items: center; gap: 4px; margin: 2px 8px 2px 0; }
.ea-legend .sw i { width: 10px; height: 10px; border-radius: 2px; display: inline-block; }
.ea-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--r); box-shadow: var(--shadow-1); min-width: 0; }
.ea-card + .ea-card { margin-top: 12px; }
.ea-card > .hd { display: flex; align-items: baseline; gap: 8px; padding: 12px 14px 4px; flex-wrap: wrap; }
.ea-card > .hd h3 { margin: 0; font-size: 14px; font-weight: 650; }
.ea-card > .hd .sub { color: var(--muted); font-size: 12px; }
.ea-card > .hd .right { margin-left: auto; }
.ea-card > .bd { padding: 6px 14px 14px; }
.ea-list > div { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 2px 10px; padding: 9px 14px; border-top: 1px solid var(--line); cursor: pointer; border-left: 3px solid transparent; }
.ea-list > div:hover { background: var(--surface-2); }
.ea-list > div.bad { border-left-color: var(--crit); }
.ea-list > div.warn { border-left-color: var(--warn); }
.ea-list > div.info { border-left-color: var(--line-2); }
.ea-list .t { font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ea-list .m { color: var(--muted); font-size: 12px; grid-column: 1 / 3; }
.ea-list .v { font-variant-numeric: tabular-nums; color: var(--text-2); font-size: 12.5px; text-align: right; white-space: nowrap; }
.ea-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; }
.ea-grid.small { grid-template-columns: repeat(auto-fill, minmax(112px, 1fr)); gap: 8px; }
.ea-tile { border: 1px solid var(--line); border-radius: var(--r-sm); overflow: hidden; background: var(--surface); cursor: pointer; min-width: 0; text-decoration: none; color: inherit; display: block; }
.ea-tile:hover { border-color: var(--accent); text-decoration: none; }
.ea-tile img { width: 100%; aspect-ratio: 1; display: block; object-fit: cover; background: #23302a; }
.ea-tile .c { padding: 6px 8px 7px; font-size: 12px; line-height: 1.35; }
.ea-tile .c b { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
.ea-tile .c span { color: var(--muted); display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ea-tile.on { outline: 2px solid var(--accent); }
.ea-pairs { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; }
.ea-pair { border: 1px solid var(--line); border-radius: var(--r); background: var(--surface); overflow: hidden; }
.ea-pair .two { display: grid; grid-template-columns: 1fr 1fr; gap: 2px; background: var(--line); }
.ea-pair img { width: 100%; aspect-ratio: 1; display: block; object-fit: cover; cursor: pointer; background: #23302a; }
.ea-pair .lbl { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; padding: 7px 9px 9px; font-size: 12px; }
.ea-pair .lbl b { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12.5px; }
.ea-pair .lbl span { color: var(--muted); }
.ea-pair .sim { padding: 0 9px 9px; font-size: 11.5px; color: var(--muted); display: flex; gap: 8px; align-items: center; }
.ea-chips { display: flex; flex-wrap: wrap; gap: 6px; margin: 6px 0 10px; }
.ea-chip { display: inline-flex; align-items: center; gap: 5px; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--line-2); background: var(--surface); font-size: 12.5px; cursor: pointer; white-space: nowrap; color: var(--text-2); }
.ea-chip.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); }
.ea-chip i { width: 9px; height: 9px; border-radius: 2px; display: inline-block; }
.ea-chip .c { color: var(--muted); font-size: 11.5px; }
.ea-search { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.ea-search input[type=search], .ea-search input[type=text] { flex: 1 1 260px; min-width: 0; height: 36px; border: 1px solid var(--line-2); border-radius: var(--r-sm); padding: 0 12px; background: var(--surface); }
.ea-drop { border: 2px dashed var(--line-2); border-radius: var(--r); padding: 26px; text-align: center; color: var(--text-2); cursor: pointer; }
.ea-drop.over { border-color: var(--accent); background: var(--accent-soft); }
.ea-drop img { max-width: 220px; max-height: 220px; border-radius: 8px; display: block; margin: 0 auto 8px; }
.ea-kv { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 12px; font-size: 13px; }
.ea-kv dt { color: var(--muted); }
.ea-kv dd { margin: 0; font-variant-numeric: tabular-nums; }
.ea-cover { display: flex; height: 12px; border-radius: 6px; overflow: hidden; background: var(--sunken); margin: 6px 0 4px; }
.ea-cover i { display: block; height: 100%; }
.ea-coverkey { display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 11.5px; color: var(--text-2); }
.ea-coverkey i { width: 9px; height: 9px; border-radius: 2px; display: inline-block; margin-right: 3px; vertical-align: -1px; }
.ea-years { display: flex; gap: 2px; margin-top: 6px; }
.ea-years i { flex: 1; height: 14px; border-radius: 2px; background: var(--sunken); }
.ea-years i.on { background: var(--fire); }
.ea-hero { padding: 22px 4px 8px; }
.ea-hero h2 { font-size: clamp(26px, 4vw, 40px); margin: 0; letter-spacing: -.02em; line-height: 1.1; }
.ea-hero h2 em { font-style: normal; color: var(--fire); }
.ea-hero p { color: var(--text-2); font-size: 15.5px; max-width: 780px; margin: 10px 0 14px; line-height: 1.5; }
.ea-two { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 14px; }
.ea-three { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 14px; }
.ea-region { padding: 14px; cursor: pointer; }
.ea-region h3 { margin: 0 0 4px; font-size: 17px; display: flex; gap: 8px; align-items: center; }
.ea-big { font-size: 30px; font-weight: 700; letter-spacing: -.02em; }
.ea-num { font-variant-numeric: tabular-nums; }
.ea-note { color: var(--muted); font-size: 12px; }
.ea-sentence { font-size: 14.5px; line-height: 1.5; }
.ea-sentence b { color: var(--fire-ink); }
.ea-play { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-top: 10px; }
.ea-play input[type=range] { flex: 1 1 240px; }
.ea-sql { position: fixed; right: 0; top: 0; bottom: 0; width: min(640px, 100vw); background: var(--surface); border-left: 1px solid var(--line); box-shadow: var(--shadow-3); z-index: 1000; overflow: auto; padding: 16px; }
.ea-sql pre { white-space: pre-wrap; word-break: break-word; background: var(--code); padding: 8px 10px; border-radius: 6px; font-size: 12px; margin: 4px 0 12px; }
.ea-sql h4 { margin: 0; font-size: 13px; }
.ea-svg { width: 100%; height: auto; display: block; }
.ea-svg text { font-size: 10px; fill: var(--muted); }
.ea-err { color: var(--crit-ink); background: var(--crit-soft); border-radius: var(--r-sm); padding: 10px 12px; }
.ea-empty { color: var(--muted); padding: 14px 0; }
.ea table { border-collapse: collapse; width: 100%; font-size: 13px; }
.ea th, .ea td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--line); vertical-align: top; }
.ea th { color: var(--muted); font-weight: 500; font-size: 12px; }
.ea td.num, .ea th.num { text-align: right; font-variant-numeric: tabular-nums; }
.ea code { font-family: var(--mono); font-size: 12px; }
.ea-mix { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin: 8px 0; }
.ea-mix input { height: 32px; border: 1px solid var(--line-2); border-radius: var(--r-sm); padding: 0 10px; background: var(--surface); min-width: 0; }
.ea-wcards { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 10px; }
.ea-wcard { display: block; border: 1px solid var(--line); border-left: 4px solid var(--good); border-radius: var(--r-sm); padding: 10px 12px; background: var(--surface); color: inherit; text-decoration: none; min-width: 0; }
a.ea-wcard:hover { border-color: var(--accent); text-decoration: none; }
div > .ea-wcard + .ea-wcard { margin-top: 10px; }
.ea-wcards > .ea-wcard + .ea-wcard { margin-top: 0; }
.ea-wcard.s-warn { border-left-color: var(--warn); }
.ea-wcard.s-alert { border-left-color: var(--crit); background: var(--crit-soft); }
.ea-wcard .nm { display: flex; gap: 8px; align-items: center; justify-content: space-between; }
.ea-wcard .nm b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ea-wcard .say { margin-top: 6px; font-size: 14px; font-weight: 550; }
.ea-wcard .meta { display: flex; flex-wrap: wrap; gap: 4px 12px; margin-top: 8px; font-size: 12px; color: var(--muted); }
.ea-wcard .meta .off { color: var(--crit-ink); }
.ea-pill { font-size: 11.5px; font-weight: 600; padding: 1px 9px; border-radius: 999px; white-space: nowrap; }
.ea-pill.good { background: var(--good-soft); color: var(--good-ink); }
.ea-pill.warn { background: var(--warn-soft); color: var(--text); }
.ea-pill.crit { background: var(--crit); color: #fff; }
.ea-pill.mute { background: var(--surface-3); color: var(--text-2); }
.ea-skybars { width: 120px; height: 22px; display: inline-block; vertical-align: middle; }
.ea-skyrow { display: inline-flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.ea-skyline svg { width: 90px; height: 16px; vertical-align: -3px; }
.ea-whead { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin: 4px 0 12px; }
.ea-whead h2 { margin: 0; font-size: 22px; letter-spacing: -.01em; }
.ea-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 7px 0; border-top: 1px solid var(--line); flex-wrap: wrap; }
.ea-row:first-child { border-top: 0; }
.ea-sw { position: relative; display: inline-block; width: 36px; height: 20px; flex: none; }
.ea-sw input { opacity: 0; width: 0; height: 0; position: absolute; }
.ea-sw span { position: absolute; inset: 0; background: var(--line-2); border-radius: 999px; cursor: pointer; transition: background .15s; }
.ea-sw span::after { content: ""; position: absolute; left: 3px; top: 3px; width: 14px; height: 14px; border-radius: 50%; background: #fff; transition: transform .15s; }
.ea-sw input:checked + span { background: var(--accent); }
.ea-sw input:checked + span::after { transform: translateX(16px); }
.ea-sw input:focus-visible + span { outline: 2px solid var(--focus); outline-offset: 2px; }
.ea-banner { display: flex; align-items: center; gap: 10px 14px; flex-wrap: wrap; padding: 10px 14px; border: 1px solid var(--line); border-radius: var(--r); background: var(--warn-soft); margin-bottom: 12px; }
.ea-banner .tag { font-size: 11px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--fire-ink); }
@media (max-width: 600px) { .ea-head .sub { display: none; } .ea-map { height: 56vh; } }
`;

// ------------------------------------------------------------------ helpers

function h(tag, attrs, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === "style" && typeof v === "object") Object.assign(e.style, v);
    else if (k.startsWith("on") && typeof v === "function") e.addEventListener(k.slice(2), v);
    else if (k === "html") e.innerHTML = v;
    else e.setAttribute(k, v === true ? "" : v);
  }
  for (const k of kids.flat(Infinity)) if (k != null && k !== false) e.append(k instanceof Node ? k : String(k));
  return e;
}
/// Appends, or replaces with, the children that are there (a card left
/// out is null; `append` would write "null").
function put(el, ...kids) { el.append(...kids.flat(Infinity).filter((k) => k != null && k !== false)); }
function swap(el, ...kids) { el.replaceChildren(...kids.flat(Infinity).filter((k) => k != null && k !== false)); }
function svg(tag, attrs, ...kids) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs || {})) if (v != null) e.setAttribute(k, v);
  for (const k of kids.flat(Infinity)) if (k != null) e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  return e;
}
const fmt = (x, d = 0) => (x == null || Number.isNaN(+x) ? "–" : (+x).toLocaleString("en-US", { maximumFractionDigits: d, minimumFractionDigits: 0 }));
const km2 = (x) => (x == null ? "–" : x < 10 ? `${fmt(x, 1)} km²` : `${fmt(x)} km²`);
const pct = (x) => (x == null ? "–" : `${fmt(x)}%`);
const cap = (s) => (s ? s[0].toUpperCase() + s.slice(1) : "");
function when(iso, region, opt = {}) {
  if (!iso) return "–";
  const d = new Date(iso);
  const tz = REGION[region]?.tz;
  return d.toLocaleString("en-US", { timeZone: tz, month: "short", day: "numeric", year: opt.year ? "numeric" : undefined, hour: opt.date ? undefined : "numeric", minute: opt.date ? undefined : "2-digit", timeZoneName: opt.date ? undefined : "short" });
}
function ago(s) {
  if (s == null || s < 0) return "never";
  if (s < 90) return `${Math.round(s)} s ago`;
  if (s < 5400) return `${Math.round(s / 60)} min ago`;
  if (s < 172800) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} days ago`;
}
function placeName(p) {
  const bits = [p.town || p.adm2, p.adm1].filter(Boolean);
  return [...new Set(bits)].join(", ") || `${fmt(p.lat, 2)}, ${fmt(p.lon, 2)}`;
}
function thumbUrl(lat, lon, px = 192) {
  const dl = HALF / Math.cos((lat * Math.PI) / 180);
  const bbox = [lon - dl, lat - HALF, lon + dl, lat + HALF].map((x) => x.toFixed(5)).join(",");
  return `https://tiles.maps.eox.at/wms?service=WMS&request=GetMap&version=1.1.1&layers=s2cloudless&styles=&srs=EPSG:4326&bbox=${bbox}&width=${px}&height=${px}&format=image/jpeg`;
}
const ICON = '<circle cx="12" cy="12" r="9"/><path d="M3.6 9h16.8M3.6 15h7"/><path d="M17 21c-2 0-3.5-1.4-3.5-3.3 0-1.6 1.2-2.6 1.9-3.7.4 1 1.1 1.4 1.6 1.4 0-1.2.4-2.4 1.4-3.4.2 1.8 2.1 2.9 2.1 5.4 0 2.1-1.5 3.6-3.5 3.6z"/>';

let mlPromise = null;
function maplibre() {
  if (window.maplibregl) return Promise.resolve(window.maplibregl);
  if (mlPromise) return mlPromise;
  mlPromise = new Promise((ok, no) => {
    document.head.append(h("link", { rel: "stylesheet", href: `${MAPLIBRE}.css` }));
    const s = h("script", { src: `${MAPLIBRE}.js` });
    s.onload = () => ok(window.maplibregl);
    s.onerror = () => { mlPromise = null; no(new Error("the map library did not load (cdn.jsdelivr.net)")); };
    document.head.append(s);
  });
  return mlPromise;
}

function style() {
  return {
    version: 8,
    sources: {
      s2: { type: "raster", tiles: [EOX], tileSize: 256, maxzoom: 15, attribution: ATTRIB },
      labels: { type: "raster", tiles: [LABELS], tileSize: 256, maxzoom: 15 },
      dem: { type: "raster-dem", tiles: [DEM], tileSize: 256, maxzoom: 14, encoding: "terrarium" },
    },
    layers: [
      { id: "s2", type: "raster", source: "s2" },
      { id: "labels", type: "raster", source: "labels", paint: { "raster-opacity": 0.85 } },
    ],
  };
}

/// A map in `box`, with buttons for labels and 3D. Resolves to the map
/// once its style loaded.
async function makeMap(box, o = {}) {
  const ml = await maplibre();
  const map = new ml.Map({ container: box, style: style(), center: o.center || [-100, 10], zoom: o.zoom ?? 3, attributionControl: { compact: true }, maxPitch: 75, dragRotate: true });
  map.addControl(new ml.NavigationControl({ visualizePitch: true }), "bottom-right");
  map.addControl(new ml.ScaleControl({ unit: "metric" }), "bottom-right");
  await new Promise((ok) => map.once("load", ok));
  // The page lays itself out after the map is made: follow its size.
  const ro = new ResizeObserver(() => map.resize());
  ro.observe(box);
  map.on("remove", () => ro.disconnect());
  map.resize();
  if (o.bounds) map.fitBounds(o.bounds, { padding: 20, duration: 0 });
  const tr = h("div", { class: "ea-maptr" });
  const b3 = h("button", { class: "ea-mapbtn", title: "Tilt the map over the terrain" }, "3D");
  b3.onclick = () => {
    const on = !b3.classList.contains("on");
    b3.classList.toggle("on", on);
    if (on) { map.setTerrain({ source: "dem", exaggeration: 1.4 }); map.easeTo({ pitch: 62, duration: 900 }); }
    else { map.setTerrain(null); map.easeTo({ pitch: 0, duration: 600 }); }
  };
  const bl = h("button", { class: "ea-mapbtn on", title: "Show or hide place names" }, "Labels");
  bl.onclick = () => { const on = !bl.classList.contains("on"); bl.classList.toggle("on", on); map.setLayoutProperty("labels", "visibility", on ? "visible" : "none"); };
  tr.append(...(o.buttons || []), bl, b3);
  box.append(tr);
  // On a phone the credits start folded, behind their (i), so they cover no map.
  if (matchMedia("(max-width: 640px)").matches) box.querySelector(".maplibregl-ctrl-attrib")?.classList.remove("maplibregl-compact-show");
  map.ea = { tr };
  return map;
}

function setData(map, id, fc, layers) {
  const src = map.getSource(id);
  if (src) { src.setData(fc); return; }
  map.addSource(id, { type: "geojson", data: fc });
  for (const l of layers) map.addLayer({ ...l, source: id }, l.before);
}

function squares(lat, lon, half) {
  const f = [];
  for (let i = 0; i < lat.length; i++) {
    const dl = half / Math.cos((lat[i] * Math.PI) / 180);
    f.push([lat[i], lon[i], dl]);
  }
  return f;
}
const square = (la, lo, hl, dl) => [[[lo - dl, la - hl], [lo + dl, la - hl], [lo + dl, la + hl], [lo - dl, la + hl], [lo - dl, la - hl]]];

/// Fire pixels as heat: one field summed over the pixels near each other
/// (a MapLibre heatmap), so a fire reads as one burning area, hottest where
/// its pixels are freshest and most powerful, and fading with their age.
function glowFeatures(rows) {
  // A spot is seen again every scan: keep its freshest, hottest look, so
  // heat does not pile up where a fire merely lasted.
  const spots = new Map();
  for (const p of rows) {
    const fresh = Math.max(0, 1 - (p.age ?? 0));
    const w = fresh ** 1.5 * (0.45 + 0.55 * Math.sqrt(Math.min(1, (p.mw || 0) / 250)));
    const k = `${p.lat.toFixed(3)},${p.lon.toFixed(3)}`;
    const o = spots.get(k);
    if (!o || o.w < w) spots.set(k, { w, lat: p.lat, lon: p.lon });
  }
  return {
    type: "FeatureCollection",
    features: [...spots.values()].map((p) => ({ type: "Feature", properties: { w: p.w }, geometry: { type: "Point", coordinates: [p.lon, p.lat] } })),
  };
}
const GLOW_LAYERS = [
  { id: "px-heat", type: "heatmap", paint: {
    "heatmap-weight": ["get", "w"],
    // About four GOES pixels (2 to 3 km apart) in screen pixels at each zoom,
    // so that neighbours run together.
    "heatmap-radius": ["interpolate", ["exponential", 2], ["zoom"], 4, 7, 8, 15, 10, 56, 13, 450, 16, 3600],
    "heatmap-intensity": ["interpolate", ["linear"], ["zoom"], 5, 1.3, 10, 1.05, 13, 1],
    "heatmap-color": ["interpolate", ["linear"], ["heatmap-density"],
      0, "rgba(0,0,0,0)", 0.08, "rgba(110,0,0,0)", 0.2, "rgba(150,12,0,.55)", 0.4, "rgba(215,40,0,.8)",
      0.6, "rgba(255,110,0,.9)", 0.8, "rgba(255,190,40,.95)", 1, "rgba(255,245,200,1)"],
    "heatmap-opacity": 0.95 } },
];
/// A button over a map that hides GOES's fire pixels, so the fire shows as
/// the picture under them has it.
function pixelButton(map) {
  map.ea.pxOn = true;
  const b = h("button", { class: "ea-mapbtn on", title: "Show or hide GOES's fire pixels, to see the fire in the satellite's own picture" }, "Fire pixels");
  b.onclick = () => {
    map.ea.pxOn = !map.ea.pxOn;
    b.classList.toggle("on", map.ea.pxOn);
    for (const l of GLOW_LAYERS) if (map.getLayer(l.id)) map.setLayoutProperty(l.id, "visibility", map.ea.pxOn ? "visible" : "none");
  };
  map.ea.tr.prepend(b);
}
const glowLayers = (map) => GLOW_LAYERS.map((l) => ({ ...l, layout: { visibility: map.ea?.pxOn === false ? "none" : "visible" } }));

function ramp(name) {
  return RAMP[name] || RAMP.default;
}

// ------------------------------------------------------------------ the view

function render(root, ctx) {
  root.classList.add("ea");
  if (!document.getElementById("ea-css")) document.head.append(h("style", { id: "ea-css" }, CSS));
  const app = { ctx, last: null, stops: [], cache: new Map() };
  const api = async (path, o = {}) => {
    const hit = !o.fresh && !o.body && app.cache.get(path);
    if (hit && Date.now() - hit.t < (o.keep || 20000)) { app.last = hit.j; return hit.j; }
    const opts = { timeout: o.timeout || 60000 };
    if (o.body) Object.assign(opts, { method: "POST", body: JSON.stringify(o.body), type: "application/json" });
    const j = await ctx.api(path, opts);
    if (!o.body) app.cache.set(path, { t: Date.now(), j });
    app.last = j;
    return j;
  };
  app.api = api;
  app.href = (...parts) => ctx.href(...parts.filter((x) => x !== "" && x != null));
  app.go = (...parts) => { location.hash = app.href(...parts); };
  // A page that belongs under another tab than its path says (a burning
  // fire under Live) marks that one.
  app.markTab = (k) => { for (const a of tabs.querySelectorAll("a")) a.classList.toggle("on", a.getAttribute("href") === app.href(k)); };

  const head = h("div", { class: "ea-head" },
    h("h1", { html: `<svg viewBox="0 0 24 24">${ICON}</svg>Earth` }),
    h("span", { class: "sub" }, "California and Chile from space, on ChloeDB"),
    h("div", { class: "tools" }, h("button", { class: "btn sm", onclick: () => showSql(app) }, "SQL")));
  const tabs = h("nav", { class: "ea-tabs" });
  const body = h("div");
  root.append(head, tabs, body);

  const TABS = [["", "Home"], ["live", "Live"], ["watch", "Fire watch"], ["sky", "Clear sky"], ["future", "Future climate"], ["land", "Land finder"], ["explore", "Explore"], ["search", "Search"], ["twins", "Climate twins"], ["changed", "Changed places"], ["fires", "Past fires"], ["rules", "Rules"], ["data", "How it's built"]];
  const drawTabs = (p) => {
    tabs.replaceChildren(...TABS.map(([k, t]) => {
      const on = (p[0] || "") === k || (k === "explore" && p[0] === "map") || (k === "search" && p[0] === "picture") || (k === "fires" && p[0] === "fire") || (k === "live" && p[0] === "replay");
      return h("a", { href: app.href(k), class: on ? "on" : null }, t);
    }));
  };

  let token = 0;
  const show = async (p) => {
    const mine = ++token;
    for (const s of app.stops.splice(0)) { try { s(); } catch { /* gone */ } }
    drawTabs(p);
    body.replaceChildren(h("div", { class: "ea-empty" }, "Loading…"));
    const page = h("div");
    try {
      const [k, a, b] = p;
      const draw = {
        "": () => home(app, page),
        live: () => live(app, page, a || "california"),
        replay: () => replay(app, page, a),
        watch: () => (a ? watchOne(app, page, a) : watchHome(app, page)),
        sky: () => skyPage(app, page, a || "california"),
        land: () => landPage(app, page, a || "california"),
        future: () => futurePage(app, page, a || "california", b ? decodeURIComponent(b) : null),
        fire: () => fire(app, page, a),
        fires: () => pastFires(app, page, a || "california"),
        explore: () => explore(app, page, a || "california", null),
        place: () => report(app, page, a),
        map: () => explore(app, page, null, a),
        search: () => search(app, page, a || ""),
        picture: () => picture(app, page),
        twins: () => twins(app, page, a || "california", b || ""),
        changed: () => changed(app, page, a || ""),
        rules: () => rules(app, page),
        data: () => data(app, page),
      }[k || ""];
      if (!draw) throw new Error(`no page ${k}`);
      body.replaceChildren(page);
      await draw();
    } catch (e) {
      if (mine !== token) return;
      body.replaceChildren(h("div", { class: "ea-err" }, String(e.message || e)));
    }
  };
  ctx.onPath((p) => show(p));
  show(ctx.path());
  // Fire watch's notifications, on whichever page is open.
  app.notifier = watchNotifier(app);
  return () => { token++; app.notifier?.(); for (const s of app.stops.splice(0)) { try { s(); } catch { /* gone */ } } };
}

function showSql(app) {
  const q = app.last?.queries || [];
  const box = h("div", { class: "ea-sql" },
    h("div", { style: { display: "flex", alignItems: "center", gap: "8px", marginBottom: "10px" } },
      h("h3", { style: { margin: 0 } }, "The SQL behind this page"),
      h("span", { class: "ea-note" }, app.last?.took_ms != null ? `${fmt(app.last.took_ms, 1)} ms in all` : ""),
      h("button", { class: "btn sm", style: { marginLeft: "auto" }, onclick: () => box.remove() }, "Close")),
    q.length ? q.map((x) => h("div", {}, h("h4", {}, x.what, h("span", { class: "ea-note" }, `  ${fmt(x.rows)} rows, ${fmt(x.ms, 1)} ms${x.note ? `, ${x.note}` : ""}`)), h("pre", {}, x.sql + (x.params ? `\n-- ${JSON.stringify(x.params)}` : "")))) : h("p", { class: "ea-empty" }, "This page ran no SQL."));
  document.body.append(box);
  const esc = (e) => { if (e.key === "Escape") { box.remove(); removeEventListener("keydown", esc); } };
  addEventListener("keydown", esc);
}

function card(title, sub, ...kids) {
  return h("div", { class: "ea-card" }, h("div", { class: "hd" }, h("h3", {}, title), sub ? h("span", { class: "sub" }, sub) : null), h("div", { class: "bd" }, ...kids));
}

function onAir(live, text) {
  return h("div", { class: `ea-onair${live ? "" : " off"}` }, h("span", { class: "light" }), h("b", {}, live ? "LIVE" : "NOT LIVE"), h("span", { class: "t" }, text));
}

function regionSwitch(app, page, region, sub = []) {
  return h("div", { class: "ea-chips" }, Object.entries(REGION).map(([k, r]) => h("a", { class: `ea-chip${k === region ? " on" : ""}`, href: app.href(page, k, ...sub) }, r.title)));
}

function tile(app, p, extra) {
  return h("a", { class: "ea-tile", href: app.href("place", p.id), title: p.words || "" },
    h("img", { loading: "lazy", alt: placeName(p), src: thumbUrl(p.lat, p.lon) }),
    h("div", { class: "c" }, h("b", {}, placeName(p)), h("span", {}, [cap(p.kind), p.climate].filter(Boolean).join(" · ")), extra ? h("span", {}, extra) : null));
}

function fireRow(app, f, opt = {}) {
  const region = f.region;
  const sev = f.state === "burning" ? (f.peak_mw >= 1000 || f.people5 >= 10000 ? "bad" : "warn") : "info";
  return h("div", { class: sev, onclick: () => app.go("fire", f.id) },
    h("span", { class: "t" }, f.name || f.id),
    h("span", { class: "v" }, f.state === "burning" ? `${fmt(f.mw_now)} MW now` : f.pixels >= 3 ? km2(f.area_km2) : `${fmt(f.pixels)} ${f.pixels === 1 ? "pixel" : "pixels"}`),
    h("span", { class: "m" }, [
      opt.date !== false ? when(f.start, region, { year: true }) : null,
      `${fmt(f.hours, 1)} h`, `peak ${fmt(f.peak_mw)} MW`,
      f.people5 ? `${fmt(f.people5)} people within 5 km` : null,
      f.outlook_km2 != null && f.state === "burning" ? `fires like it grew to ${km2(f.outlook_km2)}` : null,
    ].filter(Boolean).join(" · ")));
}

// -------------------------------------------------------------------- home

async function home(app, page) {
  const j = await app.api("/v1/home");
  const search = h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); const q = e.target.q.value.trim(); if (q) app.go("search", q); } },
    h("input", { type: "search", name: "q", placeholder: "Search places by how they look: vineyards on hills, a lithium mine, a burn scar…" }),
    h("button", { class: "btn primary" }, "Search"),
    h("a", { class: "btn", href: app.href("picture") }, "By a picture"));
  const found = h("div", { class: "ea-list" });
  let t = 0;
  const finder = h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); const f = found.querySelector("[data-id]"); if (f) app.go("place", f.dataset.id); } },
    h("input", { type: "search", name: "q", placeholder: "Find a place: a town, a county, or 34.05, -118.2", oninput: async (e) => {
      const q = e.target.value.trim(), mine = ++t;
      if (q.length < 2) return swap(found);
      const r = await app.api(`/v1/find?q=${encodeURIComponent(q)}`, { keep: 60000 });
      if (mine === t) swap(found, r.found.length ? r.found.map((x) => h("div", { class: "info", "data-id": x.id, onclick: () => app.go("place", x.id) }, h("span", { class: "t" }, x.label), x.label.endsWith(REGION[x.region]?.title || x.region) ? null : h("span", { class: "v" }, REGION[x.region]?.title || x.region))) : h("p", { class: "ea-note" }, "No town or county of California or Chile starts with that."));
    } }),
    h("button", { type: "button", class: "btn", onclick: () => navigator.geolocation?.getCurrentPosition(async (pos) => { const at = await app.api(`/v1/at?lat=${pos.coords.latitude.toFixed(5)}&lon=${pos.coords.longitude.toFixed(5)}`, { fresh: true }); if (at.id) app.go("place", at.id); else swap(found, h("p", { class: "ea-note" }, "Your location is outside California and Chile.")); }) }, "Use my location"));
  put(page, h("div", { class: "ea-hero" },
    h("h2", {}, "What happened ", h("em", {}, "where you live?")),
    h("p", {}, "Find a place to see the fires that burned near it, the years satellites saw heat there, and how its surroundings changed."),
    finder, found));
  put(page, h("div", { class: "ea-hero" },
    h("h2", {}, "California and Chile, ", h("em", {}, "from space")),
    h("p", {}, "Every fire GOES sees, every few minutes, grouped into fires as they grow and matched to the fires that began like them. Every 3.84 km square of both, searchable by what it looks like, with what lies on it, its climate and its twin on the other side of the equator."),
    search));
  const regions = h("div", { class: "ea-two" }, j.regions.map((r) => {
    const R = REGION[r.region];
    return h("div", { class: "ea-card ea-region", onclick: () => app.go("live", r.region) },
      h("h3", {}, R.title, onAir(r.live, r.now ? `last scan ${ago((Date.now() - Date.parse(r.now)) / 1000)}` : "no scans read yet")),
      h("div", { class: "ea-big" }, `${fmt(r.burning)} `, h("span", { class: "ea-note", style: { fontSize: "14px", fontWeight: 500 } }, r.burning === 1 ? "fire burning" : "fires burning")),
      h("div", { class: "ea-note" }, `${fmt(r.places)} places · ${fmt(r.alerts)} alerts open · ${R.sat}`),
      r.fires?.length ? h("div", { class: "ea-list", style: { marginTop: "8px" } }, r.fires.map((f) => fireRow(app, f, { date: false }))) : null);
  }));
  put(page, regions);
  const replays = card("Replays", "real scans of past days, played back", h("div", { class: "ea-list" }, j.replays.map((r) => h("div", { class: "warn", onclick: () => app.go("replay", r.id) }, h("span", { class: "t" }, r.title), h("span", { class: "v" }, "Play"), h("span", { class: "m" }, r.about)))));
  const twinsCard = card("Climate twins", "the places of California that look most like a place of Chile with the same climate",
    j.twins?.length ? h("div", { class: "ea-pairs", style: { gridTemplateColumns: "1fr" } }, j.twins.slice(0, 3).map((a) => pair(app, a, (j.their_twins || []).find((b) => b.id === a.twin), a.twin_sim))) : h("p", { class: "ea-empty" }, "Twins appear once the places are loaded."),
    h("a", { href: app.href("twins") }, "All the twins →"));
  const fires = card("The largest fires GOES saw", "by the area its pixels covered", j.biggest?.length ? h("div", { class: "ea-list" }, j.biggest.map((f) => fireRow(app, f))) : h("p", { class: "ea-empty" }, "No fires read yet."));
  put(page, h("div", { class: "ea-three", style: { marginTop: "14px" } }, h("div", {}, replays, backtestCard(j.backtest)), twinsCard, fires));
}

function backtestCard(rows) {
  if (!rows?.length) return card("Fires like this one, tested", "how well it foretold fires", h("p", { class: "ea-note" }, "Measured once the fire seasons are read: each fire is matched only against fires that were out before it, and its real size compared with what they became."));
  return card("Fires like this one, tested", "measured on past fires, each matched only against fires out before it",
    h("div", { class: "ea-list" }, rows.map((r) => h("div", { class: "info" },
      h("span", { class: "t" }, `${REGION[r.region]?.title || r.region}, after ${r.hours === 1 ? "the first hour" : `${fmt(r.hours)} hours`}`),
      h("span", { class: "v" }, `${fmt(r.covered * 100)}% inside`),
      h("span", { class: "m" }, `${fmt(r.fires)} fires: the middle half of the 20 most alike held the real size ${fmt(r.covered * 100)}% of the time (all earlier fires: ${fmt(r.base_covered * 100)}%), a range ${fmt(r.base_width / Math.max(r.width, 1e-9), 1)}× narrower; rank correlation ${fmt(r.spearman, 2)}.`)))));
}

// -------------------------------------------------------------------- live

/// Satellite now on a map: the newest GOES GeoColor picture from NASA
/// GIBS, on by default, under the fire pixels and the names, with a button
/// that hides it; checked again every two minutes. Resolves to the note
/// that says how old the picture is.
async function addSky(app, map, region) {
  const R = REGION[region] || REGION.california;
  const skyNote = h("div", { class: "ea-mapttl" }, "Satellite now: finding the newest picture…");
  const bs = h("button", { class: "ea-mapbtn on", title: "Show or hide the newest GOES picture of the sky" }, "Satellite now");
  map.ea.tr.prepend(bs);
  let skyAt = null;
  const sky = async () => {
    let t;
    try { t = await skyTime(region); } catch (e) { if (!skyAt) skyNote.textContent = `Satellite now: no picture (${e.message})`; return; }
    if (t !== skyAt && !(skyAt && t < skyAt)) {
      skyAt = t;
      const src = map.getSource("sky");
      if (src) src.setTiles([skyTiles(SKY[region].layer, t)]);
      else {
        map.addSource("sky", { type: "raster", tiles: [skyTiles(SKY[region].layer, t)], tileSize: 256, maxzoom: 7, attribution: SKY_ATTRIB });
        map.addLayer({ id: "sky", type: "raster", source: "sky", layout: { visibility: bs.classList.contains("on") ? "visible" : "none" },
          paint: { "raster-opacity": ["interpolate", ["linear"], ["zoom"], 6, 0.92, 9, 0.5, 11, 0.25], "raster-fade-duration": 0 } }, "labels");
      }
    }
    skyNote.replaceChildren(h("b", {}, `${R.sat} picture`), ` of ${when(skyAt, region)}, ${ago((Date.now() - Date.parse(skyAt)) / 1000)}`);
  };
  bs.onclick = () => {
    const on = !bs.classList.contains("on");
    bs.classList.toggle("on", on);
    skyNote.style.display = on ? "" : "none";
    if (map.getLayer("sky")) map.setLayoutProperty("sky", "visibility", on ? "visible" : "none");
  };
  await sky();
  const st = setInterval(() => sky().catch(() => {}), 120000);
  app.stops.push(() => clearInterval(st));
  return skyNote;
}


async function live(app, page, region) {
  const R = REGION[region];
  if (!R) throw new Error(`no region ${region}`);
  const box = h("div", { class: "ea-map" });
  const side = h("div");
  put(page, regionSwitch(app, "live", region), h("div", { class: "ea-split" }, box, side));
  const map = await makeMap(box, { center: R.center, zoom: R.zoom, bounds: R.bounds });
  app.stops.push(() => map.remove());
  const tl = h("div", { class: "ea-maptl" });
  box.append(tl);
  const skyNote = await addSky(app, map, region);
  pixelButton(map);
  let fitted = false;
  const draw = async (fresh) => {
    const j = await app.api(`/v1/live?region=${region}`, { fresh, keep: 15000 });
    const now = Date.parse(j.now) || 0;
    const pixels = j.pixels.map((p) => ({ ...p, age: now ? Math.min(1, (now - Date.parse(p.ts)) / 3600e3) : 0 }));
    setData(map, "px", glowFeatures(pixels), glowLayers(map));
    const pts = { type: "FeatureCollection", features: j.fires.map((f) => ({ type: "Feature", properties: { id: f.id, mw: f.mw_now }, geometry: { type: "Point", coordinates: [f.lon, f.lat] } })) };
    setData(map, "fires", pts, [{ id: "fires", type: "circle", paint: { "circle-radius": ["interpolate", ["linear"], ["zoom"], 4, 5, 10, 12], "circle-color": "rgba(0,0,0,0)", "circle-stroke-color": "#fff", "circle-stroke-width": 1.6 } }]);
    if (!fitted && j.fires.length) {
      fitted = true;
      const ml = window.maplibregl;
      const b = new ml.LngLatBounds();
      j.fires.slice(0, 30).forEach((f) => b.extend([f.lon, f.lat]));
      map.fitBounds(b, { padding: 80, maxZoom: 9, duration: 0 });
    }
    tl.replaceChildren(
      h("div", { class: "ea-mapttl" }, h("b", {}, R.title), ` · ${R.sat} fire pixels, the last hour`),
      onAir(j.live, j.now ? (j.live ? `${R.sat}, a scan every ${region === "chile" ? 10 : 5} min` : `last scan ${ago(j.lag_s)}`) : "no scans read yet"),
      skyNote);
    swap(side, 
      card(`${fmt(j.fires.length)} ${j.fires.length === 1 ? "fire" : "fires"} burning`, j.now ? `as of ${when(j.now, region)}` : "",
        j.fires.length ? h("div", { class: "ea-list" }, j.fires.slice(0, 40).map((f) => fireRow(app, f, { date: false }))) : h("p", { class: "ea-empty" }, j.live ? "No fire in the last scans." : "The feed is not reading scans now.")),
      j.alerts.length ? card("Alerts", `${fmt(j.alerts.length)} open`, h("div", { class: "ea-list" }, j.alerts.map((a) => h("div", { class: a.severity, onclick: () => app.go("fire", a.fire) }, h("span", { class: "t" }, a.title), h("span", { class: "v" }, ago((now - Date.parse(a.first_seen)) / 1000)), h("span", { class: "m" }, a.detail))))) : null,
      card("Scans of the last day", "fire pixels in each scan", scanChart(j.scans, region)),
      card("Replays", "", h("div", { class: "ea-list" }, j.replays.map((r) => h("div", { class: "info", onclick: () => app.go("replay", r.id) }, h("span", { class: "t" }, r.title), h("span", { class: "v" }, "Play"))))));
  };
  map.on("click", "fires", (e) => { const id = e.features?.[0]?.properties?.id; if (id) app.go("fire", id); });
  map.on("mouseenter", "fires", () => { map.getCanvas().style.cursor = "pointer"; });
  map.on("mouseleave", "fires", () => { map.getCanvas().style.cursor = ""; });
  await draw(false);
  const t = setInterval(() => draw(true).catch(() => {}), 30000);
  app.stops.push(() => clearInterval(t));
}

function scanChart(scans, region) {
  if (!scans?.length) return h("p", { class: "ea-empty" }, "No scans in the last day.");
  const W = 360, H = 70;
  const t0 = Date.parse(scans[0].ts), t1 = Date.parse(scans[scans.length - 1].ts) || t0 + 1;
  const max = Math.max(1, ...scans.map((s) => s.pixels));
  const bars = scans.map((s) => {
    const x = ((Date.parse(s.ts) - t0) / Math.max(1, t1 - t0)) * (W - 4) + 2;
    const y = H - 14 - (s.pixels / max) * (H - 20);
    return svg("rect", { x: x.toFixed(1), y: y.toFixed(1), width: 1.6, height: (H - 14 - y).toFixed(1), fill: "var(--fire)" });
  });
  return h("div", {}, svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "ea-svg" }, bars,
    svg("text", { x: 2, y: H - 2 }, when(scans[0].ts, region)), svg("text", { x: W - 2, y: H - 2, "text-anchor": "end" }, when(scans[scans.length - 1].ts, region)),
    svg("text", { x: 2, y: 9 }, `${fmt(max)} pixels`)),
  h("div", { class: "ea-note" }, `${fmt(scans.length)} scans, ${fmt(scans.reduce((a, s) => a + s.pixels, 0))} fire pixels`));
}

// ------------------------------------------------------------------ replay

async function replay(app, page, id) {
  const j = await app.api(`/v1/replay/${encodeURIComponent(id)}`, { keep: 600000 });
  const R = REGION[j.region];
  const box = h("div", { class: "ea-map" });
  const side = h("div");
  put(page, h("div", { class: "ea-split" }, h("div", {}, box, h("div", { class: "ea-play" })), side));
  const map = await makeMap(box, { center: [j.center[1], j.center[0]], zoom: j.zoom });
  app.stops.push(() => map.remove());
  const t0 = Date.parse(j.from), span = (Date.parse(j.to) - t0) / 1000;
  // The satellite of the day, which need not be the region's now.
  const sat = j.scans?.[0]?.sat ? j.scans[0].sat.replace("G", "GOES-") : R.sat;
  const px = j.pixels; // [t, lat, lon, mw, fire index]
  const speeds = [150, 300, 1200];
  const s = { t: 0, playing: true, speed: 1 };
  const tl = h("div", { class: "ea-maptl" });
  box.append(tl);
  const clock = h("b", { class: "ea-num" });
  const range = h("input", { type: "range", min: 0, max: Math.round(span), value: 0, step: 60 });
  const playBtn = h("button", { class: "btn sm" });
  const speedBtns = speeds.map((x, i) => h("button", { class: "btn sm", onclick: () => { s.speed = i; paint(); } }, `${x}×`));
  playBtn.onclick = () => { s.playing = !s.playing; if (s.playing && s.t >= span) s.t = 0; paint(); };
  range.oninput = () => { s.t = +range.value; paint(); };
  box.nextSibling.append(playBtn, ...speedBtns, range, clock);
  const firstSeen = new Map();
  for (const p of px) if (!firstSeen.has(p[4])) firstSeen.set(p[4], p[0]);
  const fires = j.fires;
  // The sky as GOES saw it at each moment (tools/frames.py's pictures):
  // the newest picture at or before the clock, the next one loaded ahead.
  const fr = (j.frames?.frames || []).map((f) => ({ ...f, at: Date.parse(f.t) }));
  const skyNote = fr.length ? h("div", { class: "ea-mapttl" }) : null;
  let skyOn = true, skyN = -1, skyWant = -1;
  const frameUrl = (n) => mediaSrc(app, `/v1/frame/${encodeURIComponent(j.id)}/${n}`);
  const loaded = new Map();
  const load = (n) => {
    if (!loaded.has(n)) loaded.set(n, frameUrl(n).then((u) => new Promise((ok, no) => { const im = new Image(); im.onload = () => ok(u); im.onerror = no; im.src = u; })));
    return loaded.get(n);
  };
  if (fr.length) {
    const [S, N, W, E] = j.frames.box;
    const bs = h("button", { class: "ea-mapbtn on", title: "Show or hide GOES's pictures of the sky" }, "Satellite");
    bs.onclick = () => {
      skyOn = !skyOn;
      bs.classList.toggle("on", skyOn);
      skyNote.style.display = skyOn ? "" : "none";
      if (map.getLayer("sky")) map.setLayoutProperty("sky", "visibility", skyOn ? "visible" : "none");
    };
    map.ea.tr.prepend(bs);
    pixelButton(map);
    const first = await load(0).catch(() => null);
    if (first) {
      map.addSource("sky", { type: "image", url: first, coordinates: [[W, N], [E, N], [E, S], [W, S]] });
      map.addLayer({ id: "sky", type: "raster", source: "sky", paint: { "raster-opacity": 0.92, "raster-fade-duration": 0 } }, "labels");
      skyN = 0;
    }
  }
  function showSky(ms) {
    if (!fr.length || !map.getSource("sky")) return;
    let k = 0;
    while (k + 1 < fr.length && fr[k + 1].at <= ms) k++;
    const f = fr[k];
    skyNote.replaceChildren(h("b", {}, `${j.frames.sat.replace("G", "GOES-")} picture`), ` of ${when(f.t, j.region)}, ${f.day > 0.5 ? "true colour" : "infrared (night)"}`);
    if (k === skyWant) return;
    skyWant = k;
    load(k).then((u) => { if (skyWant === k && skyN !== k) { map.getSource("sky")?.updateImage({ url: u }); skyN = k; } }).catch(() => {});
    for (let a = 1; a <= 3 && k + a < fr.length; a++) load(k + a).catch(() => {});
  }
  let lastI = -1;
  const paint = () => {
    playBtn.textContent = s.playing ? "Pause" : "Play";
    speedBtns.forEach((b, i) => b.classList.toggle("primary", i === s.speed));
    range.value = Math.round(s.t);
    clock.textContent = when(new Date(t0 + s.t * 1000).toISOString(), j.region);
    // Pixels seen up to now; older fade over three hours.
    let i = 0;
    while (i < px.length && px[i][0] <= s.t) i++;
    if (i !== lastI) {
      lastI = i;
      const rows = [];
      for (let k = 0; k < i; k++) { const p = px[k]; const age = (s.t - p[0]) / 10800; if (age <= 1) rows.push({ lat: p[1], lon: p[2], mw: p[3], age }); }
      setData(map, "px", glowFeatures(rows), glowLayers(map));
    }
    showSky(t0 + s.t * 1000);
    tl.replaceChildren(h("div", { class: "ea-mapttl" }, h("b", {}, j.title), ` · ${sat} fire pixels`),
      h("div", { class: `ea-onair${s.playing ? "" : " off"}` }, h("span", { class: "light" }), h("b", {}, s.playing ? "REPLAY" : "PAUSED"), h("span", { class: "t" }, `real scans, ${speeds[s.speed]}× speed`)),
      skyNote);
    // Fires seen in two scans or more: one pixel once is often a field burnt
    // on purpose, or a hot roof.
    const seen = fires.filter((f) => { const k = j.fire_ids.indexOf(f.id); return f.scans >= 2 && k >= 0 && firstSeen.get(k) <= s.t; });
    list.replaceChildren(...(seen.length ? seen.map((f) => fireRow(app, f, { date: true })) : [h("p", { class: "ea-empty", style: { padding: "8px 14px" } }, "No fire seen yet.")]));
  };
  const list = h("div", { class: "ea-list" });
  put(side, card(j.title, `${fmt(j.scans.length)} scans, ${fmt(px.length)} fire pixels`, h("p", { class: "ea-note" }, j.about)), card("Fires as they appear", "", list), card("The scans", "fire pixels in each", scanChart(j.scans, j.region)));
  let last = performance.now();
  let raf = 0;
  const tick = (now) => {
    const dt = (now - last) / 1000;
    last = now;
    if (s.playing) {
      s.t = Math.min(span, s.t + dt * speeds[s.speed]);
      if (s.t >= span) s.playing = false;
      paint();
    }
    raf = requestAnimationFrame(tick);
  };
  paint();
  raf = requestAnimationFrame(tick);
  app.stops.push(() => cancelAnimationFrame(raf));
}

// -------------------------------------------------------------------- fire

async function fire(app, page, id, at) {
  const j = await app.api(`/v1/fire/${encodeURIComponent(id)}${at ? `?at=${at}` : ""}`);
  const f = j.fire;
  const R = REGION[f.region] || REGION.california;
  const box = h("div", { class: "ea-map short" });
  const side = h("div");
  put(page, h("div", { class: "ea-split wide" }, h("div", {}, box, h("div", { style: { marginTop: "12px" } }, firstHours(f), similarCard(app, j))), side));
  const burning = f.state === "burning";
  const map = await makeMap(box, { center: [f.lon, f.lat], zoom: burning ? 9 : 10 });
  app.stops.push(() => map.remove());
  // A fire burning now is shown as Live shows it: under Live's tab, over
  // the newest picture of the sky, its pixels read again every minute.
  let skyNote = null;
  if (burning) {
    app.markTab("live");
    skyNote = await addSky(app, map, f.region);
    const again = setInterval(async () => {
      try {
        const n = await app.api(`/v1/fire/${encodeURIComponent(id)}`, { fresh: true });
        if (n.fire.last !== f.last || n.fire.state !== f.state) { page.replaceChildren(); for (const s of app.stops.splice(0)) { try { s(); } catch { /* gone */ } } fire(app, page, id, at); }
      } catch { /* the next minute */ }
    }, 60000);
    app.stops.push(() => clearInterval(again));
  }
  const t0 = Date.parse(f.start), t1 = Date.parse(f.last) || t0 + 1;
  // Its heat over its life: the newest pixels hottest, the first ones still
  // there, fainter.
  const rows = j.pixels.map((p) => ({ ...p, age: Math.min(1, (t1 - Date.parse(p.ts)) / Math.max(3 * 3600e3, t1 - t0)) * 0.75 }));
  pixelButton(map);
  setData(map, "px", glowFeatures(rows), glowLayers(map));
  if (rows.length) {
    const b = new window.maplibregl.LngLatBounds();
    rows.forEach((p) => b.extend([p.lon, p.lat]));
    map.fitBounds(b, { padding: 60, maxZoom: burning ? 10 : 12, duration: 0 });
  }
  const fresh = Date.now() - Date.parse(f.last) < 3 * 3600e3;
  box.append(h("div", { class: "ea-maptl" }, h("div", { class: "ea-mapttl" }, h("b", {}, f.name), " · its heat as GOES saw it, the newest brightest"),
    burning ? onAir(fresh, fresh ? "burning now" : `not seen since ${ago((Date.now() - Date.parse(f.last)) / 1000)}`) : null, skyNote));
  const o = j.outlook;
  put(side, 
    card(f.name, `${f.state === "burning" ? "burning" : "out"} · ${f.adm1 || ""}`,
      h("dl", { class: "ea-kv" },
        h("dt", {}, "Seen first"), h("dd", {}, when(f.start, f.region, { year: true }), f.seen_start ? "" : " (burning before the first scan read)"),
        h("dt", {}, "Seen last"), h("dd", {}, when(f.last, f.region, { year: true })),
        h("dt", {}, "Burned"), h("dd", {}, `${fmt(f.hours, 1)} hours, in ${fmt(f.scans)} scans`),
        h("dt", {}, "Area seen burning"), h("dd", {}, `${km2(f.area_km2)} (${fmt(f.pixels)} pixels)`),
        h("dt", {}, "Strongest"), h("dd", {}, `${fmt(f.peak_mw)} MW, ${fmt(f.peak_px)} pixels at once`),
        h("dt", {}, "Spread"), h("dd", {}, `${fmt(f.spread_km, 1)} km from where it started`),
        h("dt", {}, "People within 5 km"), h("dd", {}, fmt(f.people5)),
        h("dt", {}, "Where it started"), h("dd", {}, `trees ${pct(f.trees)}, shrubs ${pct(f.shrubs)}, grass ${pct(f.grass)}, built ${pct(f.built)}; slope ${fmt(f.slope, 1)}°; ${f.climate ? `${f.climate} (${KOPPEN_WORDS[f.climate] || ""})` : ""}`),
        h("dt", {}, "Burned before"), h("dd", {}, f.burned_before ? `last in ${f.burned_before}` : "not since 2001")),
      h("p", { style: { marginTop: "10px", display: "flex", gap: "14px", flexWrap: "wrap" } },
        burning ? h("a", { href: app.href("live", f.region) }, `← All fires burning in ${R.title}`) : null,
        j.place ? h("a", { href: app.href("place", j.place.id) }, "The place it started on →") : null)),
    o ? card("Fires like this one", `its first ${o.words}`,
      h("p", { class: "ea-sentence" }, `The ${o.n} fires whose first ${o.words} were most like this one's grew to `, h("b", {}, km2(o.median)), ` (half of them between ${km2(o.lo)} and ${km2(o.hi)}) and burned about ${fmt(o.hours_median, 1)} hours; ${fmt(o.over10 * 100)}% grew past 10 km².`),
      h("p", { class: "ea-note" }, `Only fires out by ${when(o.known, f.region, { year: true })} count${f.state === "out" ? ", what was known when this fire was that old" : ""}. ${f.state === "out" ? `It became ${km2(f.area_km2)}.` : ""}`),
      h("div", { class: "ea-chips" }, [1, 2, 6].map((x) => h("button", { class: `ea-chip${o.hours === x ? " on" : ""}`, onclick: () => { page.replaceChildren(); fire(app, page, id, x); } }, x === 1 ? "first hour" : `first ${x} hours`)))) : card("Fires like this one", "", h("p", { class: "ea-note" }, !f.seen_start ? "GOES did not see this fire start, so its first hours are not known." : f.hours < 1 ? "Too early: a fire is compared once GOES watched its first hour." : "Not enough fires out before it to compare with yet: they come with the fire seasons read.")),
    j.alerts?.length ? card("Its alerts", "", h("div", { class: "ea-list" }, j.alerts.map((a) => h("div", { class: a.severity }, h("span", { class: "t" }, a.title), h("span", { class: "v" }, a.state), h("span", { class: "m" }, a.detail))))) : null,
    j.backtest?.length ? backtestCard(j.backtest) : null);
}

function firstHours(f) {
  let bins = [];
  try { bins = JSON.parse(f.first || "[]"); } catch { /* none */ }
  if (!bins.length) return card("Its first hours", "", h("p", { class: "ea-empty" }, "Not watched from its start."));
  const W = 560, H = 150, n = bins.length;
  const maxMw = Math.max(1, ...bins.map((b) => b[0]));
  const maxPx = Math.max(1, ...bins.map((b) => b[1]));
  const x = (i) => 30 + (i / Math.max(1, n - 1)) * (W - 40);
  const line = bins.map((b, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${(H - 20 - (b[0] / maxMw) * (H - 34)).toFixed(1)}`).join("");
  const bars = bins.map((b, i) => svg("rect", { x: (x(i) - 3).toFixed(1), y: (H - 20 - (b[1] / maxPx) * (H - 34) * 0.5).toFixed(1), width: 6, height: ((b[1] / maxPx) * (H - 34) * 0.5).toFixed(1), fill: "var(--line-2)" }));
  const ticks = [0, 4, 8, 16, 24, 32, 40].filter((i) => i < n - 2).map((i) => svg("text", { x: x(i), y: H - 6, "text-anchor": "middle" }, `${i / 4}h`));
  return card("Its first hours", "power (line) and pixels (bars), each quarter hour",
    svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "ea-svg" }, bars, svg("path", { d: line, fill: "none", stroke: "var(--fire)", "stroke-width": 2 }), ticks,
      svg("text", { x: 2, y: 12 }, `${fmt(maxMw)} MW`), svg("text", { x: W - 2, y: 12, "text-anchor": "end" }, `${fmt(maxPx)} px`)));
}

function similarCard(app, j) {
  if (!j.similar?.length) return null;
  const me = j.fire.area_km2;
  return card("The fires whose start was most like it", "and what they became",
    h("div", { class: "ea-list" }, j.similar.map((s) => h("div", { class: "info", onclick: () => app.go("fire", s.id) },
      h("span", { class: "t" }, s.name), h("span", { class: "v" }, km2(s.area_km2)),
      h("span", { class: "m" }, `${when(s.start, s.region, { year: true, date: true })} · ${fmt(s.hours, 1)} h · peak ${fmt(s.peak_mw)} MW · ${fmt(s.people5)} people within 5 km${me != null && j.fire.state === "out" ? (s.area_km2 > me ? " · grew larger" : " · stayed smaller") : ""}`)))));
}

// ---------------------------------------------------------------- past fires

async function pastFires(app, page, region) {
  const params = new URLSearchParams(location.hash.split("?")[1] || "");
  const sort = params.get("sort") || "area";
  const year = params.get("year") || "";
  const j = await app.api(`/v1/fires?region=${region}&sort=${sort}${year ? `&year=${year}` : ""}`);
  const sorts = [["area", "Largest"], ["recent", "Most recent"], ["power", "Strongest"], ["people", "Most people near"]];
  const link = (s, y) => `${app.href("fires", region)}?sort=${s}${y ? `&year=${y}` : ""}`;
  const years = j.years.map((y) => ({ y: +String(y.year).slice(0, 4), n: y.fires, km2: y.km2 }));
  const max = Math.max(1, ...years.map((y) => y.km2 || 0));
  put(page, regionSwitch(app, "fires", region),
    card("Fires a year", "the area GOES saw burning, fires of 1 km² or more", years.length ? h("div", { class: "ea-chips" }, h("a", { class: `ea-chip${year ? "" : " on"}`, href: link(sort, "") }, "All years"), years.map((y) => h("a", { class: `ea-chip${String(y.y) === year ? " on" : ""}`, href: link(sort, y.y), title: `${fmt(y.n)} fires` }, h("i", { style: { background: "var(--fire)", opacity: 0.25 + 0.75 * ((y.km2 || 0) / max) } }), String(y.y), h("span", { class: "c" }, km2(y.km2))))) : h("p", { class: "ea-empty" }, "No fires read yet.")),
    card(`${fmt(j.fires.length)} fires`, "", h("div", { class: "ea-chips" }, sorts.map(([k, t]) => h("a", { class: `ea-chip${k === sort ? " on" : ""}`, href: link(k, year) }, t))), h("div", { class: "ea-list" }, j.fires.map((f) => fireRow(app, f)))));
}

// ------------------------------------------------------------------ explore

async function explore(app, page, region, placeId) {
  let place = null;
  if (placeId) {
    place = await app.api(`/v1/place/${encodeURIComponent(placeId)}`);
    region = place.place.region;
  }
  const R = REGION[region];
  if (!R) throw new Error(`no region ${region}`);
  const params = new URLSearchParams(location.hash.split("?")[1] || "");
  let layerName = params.get("layer") ?? (placeId ? "" : "fires");
  const box = h("div", { class: "ea-map" });
  const side = h("div");
  const chips = h("div", { class: "ea-chips" });
  put(page, regionSwitch(app, "explore", region), chips, h("div", { class: "ea-split wide" }, box, side));
  const p0 = place?.place;
  const map = await makeMap(box, { center: p0 ? [p0.lon, p0.lat] : R.center, zoom: p0 ? 11.5 : R.zoom, bounds: p0 ? null : R.bounds });
  app.stops.push(() => map.remove());
  const legend = h("div", { class: "ea-legend", style: { display: "none" } });
  box.append(legend, h("div", { class: "ea-maptl" }, h("div", { class: "ea-mapttl" }, h("b", {}, R.title), " · click a place")));
  const drawLayer = async () => {
    chips.replaceChildren(...LAYER_LIST.filter(([k]) => k !== "crop" || region === "california").map(([k, t]) => h("button", { class: `ea-chip${k === layerName ? " on" : ""}`, onclick: () => { layerName = k; drawLayer(); } }, t)));
    if (!layerName) {
      if (map.getLayer("layer")) map.setLayoutProperty("layer", "visibility", "none");
      legend.style.display = "none";
      return;
    }
    const L = await app.api(`/v1/layer/${layerName}?region=${region}`, { keep: 600000 });
    const cat = L.codes.length > 0;
    const vals = L.v.filter((x) => x != null);
    // The colours' range: the places that have any of it (most have no
    // fire, no mine, nobody), to its 98th percentile.
    const sorted = cat ? [] : vals.filter((x) => layerName === "elev" || x > 0).sort((a, b) => a - b);
    const hi = cat ? 0 : sorted[Math.floor(sorted.length * 0.98)] || 1;
    const lo = cat ? 0 : layerName === "elev" ? sorted[Math.floor(sorted.length * 0.02)] || 0 : 0;
    const colors = ramp(layerName);
    const catColor = (c) => (layerName === "climate" ? KOPPEN[c] : layerName === "kind" ? KIND[c] : null) || `hsl(${(L.codes.indexOf(c) * 47) % 360} 65% 55%)`;
    const features = [];
    const sq = squares(L.lat, L.lon, HALF);
    for (let i = 0; i < sq.length; i++) {
      const v = L.v[i];
      if (v == null || (!cat && v <= (layerName === "elev" ? -1e9 : 0) && layerName !== "elev")) continue;
      const [la, lo2, dl] = sq[i];
      features.push({ type: "Feature", properties: cat ? { c: catColor(L.codes[v]) } : { t: Math.max(0, Math.min(1, (v - lo) / Math.max(1e-9, hi - lo))) }, geometry: { type: "Polygon", coordinates: square(la, lo2, HALF, dl) } });
    }
    const fc = { type: "FeatureCollection", features };
    const paint = cat ? { "fill-color": ["get", "c"], "fill-opacity": 0.62 } : { "fill-color": ["interpolate", ["linear"], ["get", "t"], 0, colors[0], 0.25, colors[1], 0.5, colors[2], 0.75, colors[3], 1, colors[4]], "fill-opacity": 0.66 };
    if (map.getLayer("layer")) map.removeLayer("layer");
    if (map.getSource("layer")) map.removeSource("layer");
    map.addSource("layer", { type: "geojson", data: fc });
    map.addLayer({ id: "layer", type: "fill", source: "layer", paint }, "labels");
    legend.style.display = "";
    if (cat) {
      const counts = new Map();
      for (const v of vals) counts.set(L.codes[v], (counts.get(L.codes[v]) || 0) + 1);
      const top = [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12);
      legend.replaceChildren(h("b", {}, L.title), h("div", { class: "ea-note", style: { color: "#ddd" } }, L.about), h("div", {}, top.map(([c]) => h("span", { class: "sw" }, h("i", { style: { background: catColor(c) } }), layerName === "climate" ? `${c} ${KOPPEN_WORDS[c] || ""}` : c))));
    } else {
      legend.replaceChildren(h("b", {}, L.title), h("div", { style: { opacity: 0.85 } }, L.about), h("div", { class: "bar", style: { background: `linear-gradient(90deg, ${colors.join(",")})` } }), h("div", { class: "ends" }, h("span", {}, fmt(lo)), h("span", {}, `${fmt(hi)}+`)));
    }
  };
  const drawPlace = (j) => {
    const p = j.place;
    const dl = HALF / Math.cos((p.lat * Math.PI) / 180);
    setData(map, "sel", { type: "FeatureCollection", features: [{ type: "Feature", properties: {}, geometry: { type: "Polygon", coordinates: square(p.lat, p.lon, HALF, dl) } }] }, [{ id: "sel", type: "line", paint: { "line-color": "#fff", "line-width": 2.5 } }]);
    swap(side, ...placePanel(app, j));
  };
  map.on("click", async (e) => {
    const { lat, lng } = e.lngLat;
    try {
      const at = await app.api(`/v1/at?lat=${lat.toFixed(5)}&lon=${lng.toFixed(5)}`);
      if (!at.id) return;
      history.replaceState(null, "", app.href("map", at.id));
      const j = await app.api(`/v1/place/${encodeURIComponent(at.id)}`);
      drawPlace(j);
    } catch (err) { swap(side, h("div", { class: "ea-err" }, String(err.message || err))); }
  });
  if (place) drawPlace(place);
  else put(side, card("Click a place", "every 3.84 km square of the region", h("p", { class: "ea-note" }, "Each square is one of Major TOM's Sentinel-2 pictures, with what lies on it: its land cover, terrain, people, climate, crops, mines and the years it burned. Pick a layer above to paint the map with one of them.")));
  await drawLayer();
}

const eventLine = (app, e) => {
  const where = e.km > 0 ? `${fmt(e.km, 1)} km ${e.dir}` : "on this square";
  if (e.kind === "perimeter") {
    const days = e.date && e.contained ? Math.max(1, Math.round((Date.parse(e.contained) - Date.parse(e.date)) / 864e5)) : null;
    return [`${e.name || "Unnamed fire"}, ${e.acres ? `${fmt(e.acres)} acres` : "size unknown"}`,
      `${e.date ? e.date.slice(0, 10) : e.year} · ${where}${e.cause && e.cause.toLowerCase() !== "unknown" ? ` · cause: ${e.cause.toLowerCase()}` : ""}${days ? ` · out in ${days} day${days === 1 ? "" : "s"}` : ""}${e.burned ? ` · ${fmt(e.burned * 100)}% of this square` : ""}`, "CAL FIRE"];
  }
  if (e.kind === "goes") return [e.name || "Fire seen by GOES", `${e.date.slice(0, 10)} · ${where} · ${km2(e.km2)} seen`, "GOES"];
  if (e.kind === "change") return [`${e.town || "A place nearby"} went from ${e.from} to ${e.to}`, `${e.year} · ${where}`, "Sentinel-2"];
  return ["Satellites saw fire heat here", `${e.year}`, "FIRMS"];
};

async function report(app, page, id) {
  const j = await app.api(`/v1/place/${encodeURIComponent(id)}`);
  const p = j.place, st = j.story;
  const box = h("div", { class: "ea-map", style: { minHeight: "280px", height: "min(560px, 70vh)", position: "sticky", top: "12px", alignSelf: "start" } });
  put(page, h("div", { class: "ea-hero" }, h("h2", {}, placeName(p)), h("p", {}, st.headline),
    h("div", { class: "ea-search" }, h("a", { class: "btn", href: app.href("map", p.id) }, "Open on the map"))));
  put(page, h("div", { class: "ea-split wide" }, box,
    card("What happened here", "within 10 km, newest first (fires seen from space within 25 km)",
      st.events.length ? h("div", { class: "ea-list" }, st.events.map((e) => { const [t, m, src] = eventLine(app, e); return h("div", { onclick: e.kind === "goes" ? () => app.go("fire", e.id) : null }, h("span", { class: "t" }, t), h("span", { class: "v" }, src), h("span", { class: "m" }, m)); })) : h("p", { class: "ea-empty" }, "Nothing on record."))));
  const map = await makeMap(box, { center: [p.lon, p.lat], zoom: 10.5 });
  app.stops.push(() => map.remove());
  const dl = HALF / Math.cos((p.lat * Math.PI) / 180);
  const feats = [{ type: "Feature", properties: { me: 1 }, geometry: { type: "Polygon", coordinates: square(p.lat, p.lon, HALF, dl) } }];
  for (const o of st.outlines) { try { feats.push({ type: "Feature", properties: { burned: o.burned ? 1 : 0, name: `${o.name} ${o.year}` }, geometry: JSON.parse(o.shape) }); } catch { /* skip */ } }
  setData(map, "story", { type: "FeatureCollection", features: feats }, [
    { id: "story-fill", type: "fill", paint: { "fill-color": ["case", ["==", ["get", "me"], 1], "rgba(0,0,0,0)", ["==", ["get", "burned"], 1], "#e5484d", "#f5a524"], "fill-opacity": 0.35 } },
    { id: "story-line", type: "line", paint: { "line-color": ["case", ["==", ["get", "me"], 1], "#fff", "#ff8a3d"], "line-width": 2 } }]);
  put(page, h("div", { class: "ea-two", style: { marginTop: "14px", alignItems: "start" } }, placePanel(app, j)));
}

function coverBar(p) {
  const parts = COVER.map(([k, t, c]) => [t, c, p[k] || 0]).filter((x) => x[2] >= 1);
  return h("div", {}, h("div", { class: "ea-cover" }, parts.map(([t, c, v]) => h("i", { title: `${t} ${fmt(v)}%`, style: { width: `${v}%`, background: c } }))),
    h("div", { class: "ea-coverkey" }, parts.map(([t, c, v]) => h("span", {}, h("i", { style: { background: c } }), `${t} ${fmt(v)}%`))));
}

function placePanel(app, j) {
  const p = j.place;
  const R = REGION[p.region];
  const years = [];
  for (let y = 2001; y <= 2025; y++) years.push(y);
  const fy = new Set((p.fire_years || []).map(Number));
  const mixOut = h("div");
  const plus = h("input", { type: "text", placeholder: "+ add: vineyards, a lake…" });
  const minus = h("input", { type: "text", placeholder: "− take away: houses…" });
  const where = h("select", { class: "btn sm" }, h("option", { value: R.other }, `in ${REGION[R.other].title}`), h("option", { value: p.region }, `in ${R.title}`));
  const runMix = async (e) => {
    e?.preventDefault();
    mixOut.replaceChildren(h("p", { class: "ea-note" }, "Mixing…"));
    try {
      const m = await app.api(`/v1/mix?id=${encodeURIComponent(p.id)}&plus=${encodeURIComponent(plus.value)}&minus=${encodeURIComponent(minus.value)}&region=${where.value}`);
      const note = m.how === "look"
        ? `No place's description has \u201c${m.plus}\u201d: its picture is mixed with the look of the places a search for it finds first.`
        : `The places that look most like it${m.plus ? `, described with \u201c${m.plus}\u201d` : ""}${m.minus ? `, without \u201c${m.minus}\u201d` : ""}.`;
      mixOut.replaceChildren(h("p", { class: "ea-note" }, note),
        m.places.length ? h("div", { class: "ea-grid small" }, m.places.slice(0, 12).map((x) => tile(app, x))) : h("p", { class: "ea-note" }, "None found."));
    } catch (err) { mixOut.replaceChildren(h("div", { class: "ea-err" }, String(err.message || err))); }
  };
  return [
    skyCard(j.sky),
    climateCard(app, j.climate),
    landCard(app, p),
    card(placeName(p), `${R.title} · ${fmt(p.lat, 3)}, ${fmt(p.lon, 3)}`,
      h("img", { src: thumbUrl(p.lat, p.lon, 384), alt: placeName(p), style: { width: "100%", borderRadius: "8px", display: "block", aspectRatio: "1" } }),
      h("p", { class: "ea-note", style: { margin: "4px 0 8px" } }, `Picture: Sentinel-2 cloudless 2016 (EOX). Its vector: Major TOM's Sentinel-2 scene of ${p.taken ? when(p.taken, p.region, { year: true, date: true }) : "–"}.`),
      coverBar(p),
      h("dl", { class: "ea-kv", style: { marginTop: "8px" } },
        h("dt", {}, "What it is"), h("dd", {}, cap(p.kind)),
        h("dt", {}, "Terrain"), h("dd", {}, `${fmt(p.elev)} m (${fmt(p.elev_min)} to ${fmt(p.elev_max)}), slope ${fmt(p.slope, 1)}°`),
        h("dt", {}, "People"), h("dd", {}, fmt(p.people)),
        h("dt", {}, "Climate"), h("dd", {}, p.climate ? `${p.climate}, ${KOPPEN_WORDS[p.climate] || p.climate_group}` : "–"),
        p.crop ? [h("dt", {}, "Main crop"), h("dd", {}, `${p.crop} (${pct(p.crop_pct)} farmed)`)] : null,
        p.mine ? [h("dt", {}, "Mined"), h("dd", {}, pct(p.mine))] : null,
        h("dt", {}, "Nearest town"), h("dd", {}, p.town ? `${p.town}, ${fmt(p.town_km, 1)} km` : "–")),
      h("div", { class: "ea-note", style: { marginTop: "8px" } }, `Fire seen on it (FIRMS, 2001 to 2025): ${fy.size ? [...fy].join(", ") : "never"}`),
      h("div", { class: "ea-years", title: "2001 to 2025" }, years.map((y) => h("i", { class: fy.has(y) ? "on" : null, title: String(y) })))),
    card(`Its climate twins in ${REGION[R.other].title}`, "the places that look most like it, on the other side of the equator", h("div", { class: "ea-grid small" }, j.twins.slice(0, 9).map((x) => tile(app, x, `${x.climate === p.climate ? "same climate" : x.climate || ""}`)))),
    card("Mix it with words", "its picture, moved towards some words and away from others",
      h("form", { onsubmit: runMix }, h("div", { class: "ea-mix" }, plus, minus), h("div", { class: "ea-search" }, where, h("button", { class: "btn sm primary" }, "Find"))), mixOut),
    card(`Places in ${R.title} that look like it`, "away from it", h("div", { class: "ea-grid small" }, j.like.slice(0, 9).map((x) => tile(app, x)))),
    j.fires?.length ? card("Fires GOES saw near it", "", h("div", { class: "ea-list" }, j.fires.slice(0, 10).map((f) => fireRow(app, f)))) : null,
    j.years?.length ? card("Its years", j.change ? `${j.change.arc || ""}, moved most in ${j.change.step_year}` : "a clear scene of each dry season", h("div", { class: "ea-grid small" }, j.years.map((y) => chip(app, p.id, y.year, y.label)))) : null,
  ];
}

// -------------------------------------------------------------- fire watch

/// The browser's own key: a watch belongs to the key that made it, and the
/// key is in the address of the browser's feed. Kept in this browser only.
let memKey = null;
function watcherKey() {
  try {
    let k = localStorage.getItem("ea-watcher");
    if (!k) { k = crypto.randomUUID(); localStorage.setItem("ea-watcher", k); }
    return k;
  } catch {
    memKey ||= crypto.randomUUID();
    return memKey;
  }
}
const store = {
  get: (k, d) => { try { const v = localStorage.getItem(k); return v == null ? d : JSON.parse(v); } catch { return d; } },
  set: (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* private window */ } },
};
const wq = () => `w=${encodeURIComponent(watcherKey())}`;
const embedded = () => location.hash.startsWith("#/app/");
const SEV = { clear: "good", warn: "warn", alert: "crit" };
const pill = (s) => h("span", { class: `ea-pill ${SEV[s?.sev] || "good"}` }, s?.pill || "Clear");

const SOURCES = {
  california: [
    ["https://www.fire.ca.gov/incidents", "CAL FIRE incidents", "The fire's name, size and containment, once it is reported."],
    ["https://protect.genasys.com/", "Genasys Protect", "Evacuation zones and orders of most counties."],
    ["https://app.watchduty.org/", "Watch Duty", "Reports from people following radio and cameras."],
  ],
  chile: [
    ["https://www.conaf.cl/", "CONAF", "Chile's forest service: the fires it is fighting."],
    ["https://senapred.cl/", "SENAPRED", "Alerts and evacuations of Chile's disaster service."],
  ],
};

/// While Earth is open in a tab, its alerts as the browser's own
/// notifications: checked every minute, each told once.
function watchNotifier(app) {
  const tick = async () => {
    if (!store.get("ea-watch-notify", false) || typeof Notification === "undefined" || Notification.permission !== "granted") return;
    const j = await app.api(`/v1/watches?${wq()}`, { fresh: true });
    const since = store.get("ea-watch-since", "");
    const seen = new Set(store.get("ea-watch-seen", []));
    for (const a of (j.alerts || []).slice().reverse()) {
      if (seen.has(a.id) || (a.sent || a.t) < since) continue;
      seen.add(a.id);
      try { new Notification(a.title, { body: a.body, tag: a.id }); } catch { /* blocked */ }
    }
    store.set("ea-watch-seen", [...seen].slice(-300));
  };
  const t = setInterval(() => tick().catch(() => {}), 60000);
  tick().catch(() => {});
  return () => clearInterval(t);
}

function circle(lat, lon, km, n = 72) {
  const c = [];
  for (let i = 0; i <= n; i++) {
    const a = (i / n) * 2 * Math.PI;
    c.push([lon + (km / (111.32 * Math.cos((lat * Math.PI) / 180))) * Math.sin(a), lat + (km / 110.57) * Math.cos(a)]);
  }
  return c;
}
/// A watch's colour where several are drawn: never the fire's own.
const WATCH_COLORS = ["#2f6fe0", "#14a37a", "#8a5cf0", "#d4479a"];
function ringsFc(ws) {
  return { type: "FeatureCollection", features: ws.flatMap((w, i) => [
    { type: "Feature", properties: { kind: "ring" }, geometry: { type: "LineString", coordinates: circle(w.lat, w.lon, w.km) } },
    { type: "Feature", properties: { kind: "me", name: w.name, c: ws.length > 1 ? WATCH_COLORS[i % WATCH_COLORS.length] : WATCH_COLORS[0] }, geometry: { type: "Point", coordinates: [w.lon, w.lat] } }]) };
}
function ringsBounds(ws) {
  const b = new window.maplibregl.LngLatBounds();
  for (const w of ws) for (const c of circle(w.lat, w.lon, w.km, 16)) b.extend(c);
  return b;
}
const RING_LAYERS = [
  { id: "ring", type: "line", filter: ["==", ["get", "kind"], "ring"], paint: { "line-color": "#fff", "line-width": 1.6, "line-dasharray": [3, 2], "line-opacity": 0.9 } },
  { id: "me", type: "circle", filter: ["==", ["get", "kind"], "me"], paint: { "circle-radius": 7, "circle-color": ["get", "c"], "circle-stroke-color": "#fff", "circle-stroke-width": 2.5 } },
];

/// The nearest fire, scan by scan: a line per watch, the alert distance
/// dashed, and a cursor at `at` (seconds, the same clock as the track).
function distChart(tracks, o = {}) {
  const W = 560, H = 170, L = 30, B = 18, T = 8;
  const all = tracks.flatMap((t) => t.points);
  if (!all.length) return h("p", { class: "ea-empty" }, "No scans in these hours.");
  const x0 = o.from ?? Math.min(...all.map((p) => p[0])), x1 = o.to ?? Math.max(...all.map((p) => p[0]));
  const top = Math.max(10, Math.min(50, Math.ceil(Math.max(...tracks.map((t) => t.km * 1.5), ...all.map((p) => p[1] ?? 0)) / 5) * 5));
  const X = (t) => L + ((t - x0) / Math.max(1, x1 - x0)) * (W - L - 6);
  // Near at the bottom, the place itself on the axis.
  const Y = (k) => H - B - (Math.min(k, top) / top) * (H - B - T);
  const kids = [];
  for (let k = 0; k <= top; k += top > 25 ? 10 : 5) kids.push(svg("line", { x1: L, x2: W - 6, y1: Y(k), y2: Y(k), stroke: "var(--grid)" }), svg("text", { x: L - 4, y: Y(k) + 3, "text-anchor": "end" }, `${k}`));
  kids.push(svg("text", { x: L - 4, y: H - 4, "text-anchor": "end" }, "km"));
  tracks.forEach((t, i) => {
    const col = tracks.length > 1 ? WATCH_COLORS[i % WATCH_COLORS.length] : "var(--fire)";
    kids.push(svg("line", { x1: L, x2: W - 6, y1: Y(t.km), y2: Y(t.km), stroke: col, "stroke-dasharray": "4 3", opacity: 0.8 }));
    // A line where a fire was within 50 km, broken where none was.
    let d = "", pen = false;
    for (const [s, k] of t.points) {
      if (o.upto != null && s > o.upto) break;
      if (k == null) { pen = false; continue; }
      d += `${pen ? "L" : "M"}${X(s).toFixed(1)},${Y(k).toFixed(1)}`;
      pen = true;
    }
    kids.push(svg("path", { d, fill: "none", stroke: col, "stroke-width": 2, "stroke-linejoin": "round" }));
  });
  if (o.upto != null) kids.push(svg("line", { x1: X(o.upto), x2: X(o.upto), y1: T, y2: H - B, stroke: "var(--text-2)", "stroke-width": 1 }));
  if (o.label) kids.push(svg("text", { x: L, y: H - 4 }, o.label(x0)), svg("text", { x: W - 6, y: H - 4, "text-anchor": "end" }, o.label(x1)));
  const none = tracks.every((t) => t.points.every((p) => p[1] == null));
  return h("div", {}, svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "ea-svg", role: "img", "aria-label": "How far the nearest fire was, scan by scan" }, kids),
    none && o.upto == null ? h("p", { class: "ea-note" }, "No fire within 50 km in these hours.") : null);
}

function yearsBar(fy) {
  const years = [];
  for (let y = 2001; y <= 2025; y++) years.push(y);
  const s = new Set((fy || []).map(Number));
  return [h("div", { class: "ea-years", title: "2001 to 2025" }, years.map((y) => h("i", { class: s.has(y) ? "on" : null, title: String(y) }))),
    h("div", { class: "ea-note" }, s.size ? `Fire seen on this square (NASA FIRMS, three detections or more) in ${[...s].sort().join(", ")}.` : "No year with fire seen on this square since 2001 (NASA FIRMS).")];
}

function placeCard(pl) {
  if (!pl) return card("This place", "", h("p", { class: "ea-note" }, "Outside the squares Earth has pictures of."));
  return card("This place", [pl.town, pl.adm1].filter(Boolean).join(", "),
    h("dl", { class: "ea-kv" },
      h("dt", {}, "Ground"), h("dd", {}, `${cap(pl.kind)}: trees ${pct(pl.trees)}, shrubs ${pct(pl.shrubs)}, grass ${pct(pl.grass)}, built ${pct(pl.built)}`),
      h("dt", {}, "Slope"), h("dd", {}, `${fmt(pl.slope, 1)}°, ${fmt(pl.elev)} m up`),
      h("dt", {}, "People"), h("dd", {}, `${fmt(pl.people)} on the square`),
      h("dt", {}, "Climate"), h("dd", {}, pl.climate ? `${pl.climate}, ${KOPPEN_WORDS[pl.climate] || ""}` : "–")),
    h("div", { style: { marginTop: "8px" } }, yearsBar(pl.fire_years)));
}

function sourcesCard(region) {
  return card("What the official sources say", "",
    h("div", { class: "ea-list" }, (SOURCES[region] || SOURCES.california).map(([u, t, m]) => h("div", { class: "info", onclick: () => window.open(u, "_blank", "noopener") }, h("span", { class: "t" }, h("a", { href: u, target: "_blank", rel: "noopener" }, t)), h("span", { class: "v" }, "↗"), h("span", { class: "m" }, m)))),
    h("p", { class: "ea-note" }, "Earth sees fires from orbit, often minutes after they start, but a pixel is 2 to 3 km wide and can be a hot roof. Evacuation orders come from the county: follow them first."));
}

function alertLog(alerts, region, empty) {
  return alerts.length ? h("div", { class: "ea-list" }, alerts.map((a) => h("div", { class: a.kind === "here" || a.kind === "near" ? "bad" : a.kind === "clear" ? "info" : "warn" },
    h("span", { class: "t" }, a.title), h("span", { class: "v" }, when(a.t, region)), h("span", { class: "m" }, a.body)))) : h("p", { class: "ea-empty" }, empty);
}

async function watchHome(app, page) {
  const j = await app.api(`/v1/watches?${wq()}`, { fresh: true });
  const found = h("div", { class: "ea-list" });
  const msg = h("div");
  const add = async (body) => {
    msg.replaceChildren(h("p", { class: "ea-note" }, "Adding…"));
    try {
      const r = await app.api(`/v1/watches?${wq()}`, { body });
      app.go("watch", r.watch.id);
    } catch (e) { msg.replaceChildren(h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  let t = 0;
  const input = h("input", { type: "search", name: "q", autocomplete: "off", placeholder: "A town, a county, or 39.76, -121.62", "aria-label": "A place to watch", oninput: async (e) => {
    const q = e.target.value.trim(), mine = ++t;
    if (q.length < 2) return swap(found);
    const r = await app.api(`/v1/find?q=${encodeURIComponent(q)}`, { keep: 60000 });
    if (mine === t) swap(found, r.found.length ? r.found.map((x) => h("div", { class: "info", "data-i": "1", onclick: () => add({ name: x.label.split(",")[0], lat: x.lat, lon: x.lon }) }, h("span", { class: "t" }, x.label), h("span", { class: "v" }, "Watch"))) : h("p", { class: "ea-note" }, "No town or county of California or Chile starts with that."));
  } });
  const finder = h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); found.querySelector("[data-i]")?.click(); } },
    input, h("button", { class: "btn primary" }, "Watch this place"),
    h("button", { type: "button", class: "btn", onclick: () => {
      if (!navigator.geolocation) return msg.replaceChildren(h("div", { class: "ea-err" }, "This browser does not share its location."));
      msg.replaceChildren(h("p", { class: "ea-note" }, "Asking the browser where you are…"));
      navigator.geolocation.getCurrentPosition((pos) => add({ name: "My location", lat: +pos.coords.latitude.toFixed(4), lon: +pos.coords.longitude.toFixed(4) }), (err) => msg.replaceChildren(h("div", { class: "ea-err" }, `No location: ${err.message}`)));
    } }, "Use my location"));
  put(page, h("div", { class: "ea-hero" },
    h("h2", {}, "Is there a fire near ", h("em", {}, "a place you care about?")),
    h("p", {}, "Add a home, a parent's house or a farm. Earth checks every GOES scan, every 5 minutes over California and every 10 over Chile, and tells you when a fire is within the distance you pick, within 5 km, over the place, or coming closer."),
    finder, found, msg));
  const R = (r) => j.regions?.[r];
  const cards = j.watches.length
    ? h("div", { class: "ea-wcards" }, j.watches.map((w) => h("a", { class: `ea-wcard s-${w.status.sev}`, href: app.href("watch", w.id) },
      h("div", { class: "nm" }, h("b", {}, w.name), pill(w.status)),
      h("div", { class: "ea-note" }, w.where),
      h("div", { class: "say" }, w.status.say),
      w.status.sub ? h("div", { class: "ea-note" }, w.status.sub) : null,
      h("div", { class: "meta" }, h("span", {}, `Alerts within ${fmt(w.km)} km`), w.alerts ? h("span", {}, `${fmt(w.alerts)} sent`) : null, R(w.region)?.live === false ? h("span", { class: "off" }, "feed not live") : null, h("span", {}, "Open →")))))
    : h("p", { class: "ea-empty" }, "No places yet. Find one above, or see how it works on a real fire below.");
  put(page, h("div", { class: "ea-split wide" },
    h("div", {}, card("Your places", `${fmt(j.watches.length)} of ${fmt(j.max)}, kept in this browser`, cards),
      card("How it works, on a real fire", "", h("div", { class: "ea-list" }, h("div", { class: "warn", onclick: () => app.go("watch", "example") },
        h("span", { class: "t" }, "The Park Fire's first 12 hours"), h("span", { class: "v" }, "Play"),
        h("span", { class: "m" }, "24 July 2024, near Chico: three made-up places and the alerts they would have had, from GOES-18's real fire pixels."))))),
    h("div", {}, notifyCard(app), card("Alerts sent", "the newest first", alertLog(j.alerts.slice(0, 12), j.watches[0]?.region || "california", "None yet.")), manyCard(app, add))));
}

function notifyCard(app) {
  const box = h("div");
  const draw = () => {
    const on = store.get("ea-watch-notify", false) && typeof Notification !== "undefined" && Notification.permission === "granted";
    const feed = `${location.origin}/v1/watches/feed/${watcherKey()}`;
    swap(box,
      h("div", { class: "ea-row" }, h("span", {}, "Notify this browser"), h("label", { class: "ea-sw" }, h("input", { type: "checkbox", checked: on || null, "aria-label": "Notify this browser", onchange: async (e) => {
        if (!e.target.checked) { store.set("ea-watch-notify", false); return draw(); }
        if (typeof Notification === "undefined") { e.target.checked = false; return swap(box, h("div", { class: "ea-err" }, "This browser has no notifications.")); }
        const p = await Notification.requestPermission();
        if (p === "granted") { store.set("ea-watch-notify", true); store.set("ea-watch-since", new Date().toISOString().replace(/\.\d+Z$/, "Z")); app.notifier?.(); app.notifier = watchNotifier(app); }
        draw();
      } }), h("span", {}))),
      h("p", { class: "ea-note" }, typeof Notification !== "undefined" && Notification.permission === "denied" ? "This browser blocks Earth's notifications: allow them in its site settings." : "While Earth is open in a tab, each alert shows as a notification. Pushes to a closed phone come later."),
      embedded() ? h("p", { class: "ea-note" }, "The feed of your alerts is on Earth's public page.") : h("div", { class: "ea-row" }, h("span", {}, "Or follow them in a feed reader:"), h("a", { href: feed, target: "_blank", rel: "noopener" }, "Atom feed")));
  };
  draw();
  return card("Tell me", "", box);
}

function manyCard(app, add) {
  const ta = h("textarea", { rows: 4, placeholder: "Pump station 3, 39.81, -121.70\nWell 12, 39.74, -121.58", style: { width: "100%", border: "1px solid var(--line-2)", borderRadius: "var(--r-sm)", padding: "8px", background: "var(--surface)" } });
  const out = h("div");
  const go = async () => {
    const lines = ta.value.split(/\n/).map((l) => l.trim()).filter(Boolean);
    let ok = 0;
    for (const l of lines) {
      const m = l.match(/^(.*?)[,;\t]\s*(-?\d+(?:\.\d+)?)\s*[,;\t]\s*(-?\d+(?:\.\d+)?)\s*$/);
      if (!m) { out.append(h("div", { class: "ea-err" }, `Not "name, lat, lon": ${l}`)); continue; }
      try { await app.api(`/v1/watches?${wq()}`, { body: { name: m[1], lat: +m[2], lon: +m[3] } }); ok++; } catch (e) { out.append(h("div", { class: "ea-err" }, `${m[1]}: ${e.message || e}`)); }
    }
    if (ok) app.go("watch");
  };
  return h("details", { class: "ea-card", style: { padding: "12px 14px" } }, h("summary", {}, h("b", {}, "Watch many places"), h("span", { class: "ea-note" }, " a list of names and coordinates")),
    h("p", { class: "ea-note" }, "One a line: a name, a latitude and a longitude, as a spreadsheet's CSV has them."), ta,
    h("div", { style: { marginTop: "8px" } }, h("button", { class: "btn sm", onclick: go }, "Watch them")), out);
}

async function watchOne(app, page, id) {
  if (id === "example") return watchExample(app, page);
  app.markTab("watch");
  const j = await app.api(`/v1/watch/${encodeURIComponent(id)}?${wq()}`, { fresh: true });
  const w = j.watch, R = REGION[w.region];
  const live = j.regions?.[w.region]?.live;
  const box = h("div", { class: "ea-map short" });
  const now = Date.parse(j.now) || Date.now();
  const st = j.status;
  put(page, h("div", { class: "ea-whead" }, h("a", { href: app.href("watch") }, "← Your places"), h("h2", {}, w.name), pill(st), h("span", { class: "ea-note" }, j.where)));
  const chart = distChart([{ km: w.km, points: j.track.map(([s, k]) => [s * 1000, k]) }], { from: now - 12 * 3600e3, to: now, label: (t) => when(new Date(t).toISOString(), w.region) });
  const reload = () => { page.replaceChildren(); for (const s of app.stops.splice(0)) { try { s(); } catch { /* gone */ } } return watchOne(app, page, id); };
  const settings = settingsCard(app, w, j.kms, reload);
  put(page, h("div", { class: "ea-split wide" },
    h("div", {}, box, card("How far the fire is, the last 12 hours", "the nearest fire pixel of the hour, scan by scan; dashed: your alert distance", chart)),
    h("div", {},
      card(st.say, st.sub || "", h("dl", { class: "ea-kv" },
        h("dt", {}, "Nearest fire"), h("dd", {}, st.km == null ? "none within 50 km" : `${fmt(st.km, 1)} km ${st.dir}`),
        st.seen ? [h("dt", {}, "Seen"), h("dd", {}, `${when(st.seen, w.region)}, ${ago((Date.now() - Date.parse(st.seen)) / 1000)}`)] : null,
        h("dt", {}, "Last scan"), h("dd", {}, j.now ? `${when(j.now, w.region)}${live ? "" : " (the feed is not live)"}` : "none read"),
        st.fire ? [h("dt", {}, "The fire"), h("dd", {}, h("a", { href: app.href("fire", st.fire) }, "Its page →"))] : null)),
      card("Alerts sent", "", alertLog(j.alerts, w.region, "None yet: a fire within the distance you picked raises one.")),
      settings)));
  put(page, h("div", { class: "ea-three", style: { marginTop: "14px" } }, sourcesCard(w.region), placeCard(j.place),
    card("Fires that started near here", "GOES since June 2023, within 25 km",
      j.fires.length ? h("div", { class: "ea-list" }, j.fires.map((f) => h("div", { class: f.state === "burning" ? "bad" : "info", onclick: () => app.go("fire", f.id) }, h("span", { class: "t" }, f.name || "A fire"), h("span", { class: "v" }, km2(f.area_km2)), h("span", { class: "m" }, `${when(f.start, w.region, { year: true, date: true })} · ${fmt(f.km, 1)} km ${f.dir} · ${fmt(f.hours, 1)} h`)))) : h("p", { class: "ea-empty" }, "None."),
      j.place?.id ? h("a", { href: app.href("place", j.place.id) }, "What happened here, since 1878 →") : null)));
  const map = await makeMap(box, { center: [w.lon, w.lat], zoom: w.km > 20 ? 8.4 : 9.6 });
  app.stops.push(() => map.remove());
  pixelButton(map);
  const rows = j.pixels.map((p) => ({ ...p, age: Math.min(1, (now - Date.parse(p.ts)) / (3 * 3600e3)) }));
  setData(map, "px", glowFeatures(rows), glowLayers(map));
  setData(map, "rings", ringsFc([w]), RING_LAYERS);
  map.fitBounds(ringsBounds([w]), { padding: 40, duration: 0, maxZoom: 11 });
  box.append(h("div", { class: "ea-maptl" }, h("div", { class: "ea-mapttl" }, h("b", {}, w.name), ` · ${R.sat} fire pixels, the last 3 hours; dashed: ${fmt(w.km)} km`), onAir(live, live ? `a scan every ${w.region === "chile" ? 10 : 5} min` : "the feed is not live")));
  const again = setInterval(async () => {
    try {
      const n = await app.api(`/v1/watch/${encodeURIComponent(id)}?${wq()}`, { fresh: true });
      if (n.now !== j.now) reload();
    } catch { /* the next minute */ }
  }, 60000);
  app.stops.push(() => clearInterval(again));
}

function settingsCard(app, w, kms, reload) {
  const out = h("div");
  const save = async (body) => {
    try { await app.api(`/v1/watch/${encodeURIComponent(w.id)}?${wq()}`, { body }); await reload(); }
    catch (e) { out.replaceChildren(h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  const remove = h("button", { class: "btn sm", onclick: async () => {
    if (!remove.dataset.armed) { remove.dataset.armed = "1"; remove.textContent = "Remove it and its alerts? Click again"; return; }
    try { await app.api(`/v1/watch/${encodeURIComponent(w.id)}?${wq()}`, { body: { remove: true } }); app.go("watch"); }
    catch (e) { out.replaceChildren(h("div", { class: "ea-err" }, String(e.message || e))); }
  } }, "Remove this place");
  const sw = (label, key) => h("div", { class: "ea-row" }, h("span", {}, label), h("label", { class: "ea-sw" }, h("input", { type: "checkbox", checked: w[key] || null, "aria-label": label, onchange: (e) => save({ [key]: e.target.checked }) }), h("span", {})));
  const name = h("input", { type: "text", value: w.name, maxlength: 60, "aria-label": "Its name" });
  return card("Tell me when", "",
    h("div", { class: "ea-row" }, h("span", {}, "A fire is within"), h("span", { class: "ea-chips", style: { margin: 0 } }, kms.map((k) => h("button", { class: `ea-chip${k === w.km ? " on" : ""}`, onclick: () => save({ km: k }) }, `${k} km`)))),
    sw("It comes 2 km closer in an hour", "closer"),
    sw("It passes 1,000 MW (large and intense)", "power"),
    sw("The night over it is clear (for the stars)", "sky"),
    h("div", { class: "ea-row" }, h("span", {}, "Within 5 km, and over the place"), h("span", { class: "ea-note" }, "always")),
    h("form", { class: "ea-search", style: { marginTop: "10px" }, onsubmit: (e) => { e.preventDefault(); save({ name: name.value }); } }, name, h("button", { class: "btn sm" }, "Rename")),
    h("div", { style: { marginTop: "10px" } }, remove),
    out);
}

async function watchExample(app, page) {
  app.markTab("watch");
  const j = await app.api("/v1/watches/example", { keep: 3600000 });
  const t0 = Date.parse(j.t0);
  const box = h("div", { class: "ea-map short" });
  const cards = h("div");
  const log = h("div");
  const chartBox = h("div");
  const clock = h("b", { class: "ea-num" });
  const range = h("input", { type: "range", min: 0, max: 720, step: 5, value: 30, "aria-label": "Minutes since the fire was first seen" });
  const playBtn = h("button", { class: "btn sm" }, "Play");
  put(page, h("div", { class: "ea-banner" }, h("span", { class: "tag" }, "Example"),
    h("span", {}, "Replaying ", h("b", {}, "the Park Fire's first 12 hours"), ", Wednesday 24 July 2024, with GOES-18's real fire pixels and the alerts Earth's rules raise on them. The three places are made up."),
    h("div", { class: "ea-play", style: { margin: 0, flex: "1 1 320px" } }, playBtn, range, clock)));
  put(page, h("div", { class: "ea-split wide" },
    h("div", {}, box, card("How far the fire was", "the nearest fire pixel of the hour, scan by scan; dashed: each place's alert distance", chartBox, h("div", { class: "ea-coverkey" }, j.watches.map((w, i) => h("span", {}, h("i", { style: { background: WATCH_COLORS[i] } }), w.name))))),
    h("div", {}, cards, card("What the phones showed", "each alert once, as it was raised, the newest first", h("div", { style: { maxHeight: "560px", overflow: "auto" } }, log)))));
  put(page, h("div", { class: "ea-three", style: { marginTop: "14px" } }, sourcesCard("california"), placeCard(j.watches[0].place),
    card("The fire", "", h("p", { class: "ea-sentence" }, "The Park Fire was started near Chico on the afternoon of 24 July 2024 and grew to 429,603 acres, California's fourth largest."), h("a", { href: app.href("fire", j.fire) }, "Its page →"))));
  const map = await makeMap(box, { center: [-121.71, 39.83], zoom: 9.6 });
  app.stops.push(() => map.remove());
  pixelButton(map);
  setData(map, "rings", ringsFc(j.watches), RING_LAYERS);
  map.fitBounds(ringsBounds(j.watches), { padding: 16, duration: 0 });
  box.append(h("div", { class: "ea-maptl" }, h("div", { class: "ea-mapttl" }, h("b", {}, "Park Fire"), " · GOES-18 fire pixels; dashed: each place's distance")));
  const s = { m: 30, playing: false };
  const scans = j.scans; // seconds from t0
  const paint = () => {
    const sec = s.m * 60;
    let i = 0;
    while (i + 1 < scans.length && scans[i + 1] <= sec) i++;
    range.value = s.m;
    playBtn.textContent = s.playing ? "Pause" : "Play";
    clock.textContent = `${when(new Date(t0 + sec * 1000).toISOString(), "california")} · ${Math.floor(s.m / 60)} h ${String(s.m % 60).padStart(2, "0")} min in`;
    const px = j.pixels.filter((p) => p[0] <= sec).map((p) => ({ lat: p[1], lon: p[2], mw: p[3], age: Math.min(1, (sec - p[0]) / 10800) }));
    setData(map, "px", glowFeatures(px), glowLayers(map));
    swap(cards, j.watches.map((w) => {
      const f = w.frames[i] || { sev: "clear", pill: "Clear", say: "No fire within 50 km." };
      return h("div", { class: `ea-wcard s-${f.sev}` }, h("div", { class: "nm" }, h("b", {}, w.name), pill(f)), h("div", { class: "ea-note" }, `${w.where} · alerts within ${fmt(w.km)} km`), h("div", { class: "say" }, f.say), f.sub ? h("div", { class: "ea-note" }, f.sub) : null);
    }));
    const sent = j.watches.flatMap((w) => w.alerts).filter((a) => Date.parse(a.t) - t0 <= sec * 1000).sort((a, b) => Date.parse(b.t) - Date.parse(a.t));
    swap(log, alertLog(sent, "california", "Nothing yet: the fire is not near enough."));
    swap(chartBox, distChart(j.watches.map((w) => ({ km: w.km, points: w.track.map(([t, k]) => [t * 1000 - t0, k]) })), { from: -1800e3, to: 720 * 60e3, upto: sec * 1000, label: (x) => when(new Date(t0 + x).toISOString(), "california") }));
  };
  range.oninput = () => { s.m = +range.value; s.playing = false; paint(); };
  playBtn.onclick = () => { s.playing = !s.playing; if (s.playing && s.m >= 720) s.m = 0; paint(); };
  const tick = setInterval(() => { if (!s.playing) return; s.m = Math.min(720, s.m + 5); if (s.m >= 720) s.playing = false; paint(); }, 400);
  app.stops.push(() => clearInterval(tick));
  paint();
}

// --------------------------------------------------------------- clear sky

const SKY_WORDS = { clear: "Clear", "partly cloudy": "Partly cloudy", cloudy: "Cloudy", smoke: "Smoke", "not seen": "Not seen" };
const SKY_SEV = { clear: "good", "partly cloudy": "warn", cloudy: "mute", smoke: "crit", "not seen": "mute" };
const skyPill = (s) => h("span", { class: `ea-pill ${SKY_SEV[s?.words] || "mute"}` }, SKY_WORDS[s?.words] || "Not seen");

/// The last 30 days (or nights) as bars: the taller, the clearer; grey
/// where none was read.
function skyBars(v, night) {
  const W = 120, H = 22, n = v.length || 30, bw = W / n;
  return svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "ea-skybars", role: "img", "aria-label": `${night ? "Nights" : "Days"} of the last 30, by how clear` },
    v.map((x, i) => svg("rect", { x: (i * bw + 0.5).toFixed(1), width: (bw - 1).toFixed(1), y: x < 0 ? H - 2 : (H - Math.max(2, (x / 100) * H)).toFixed(1), height: x < 0 ? 2 : Math.max(2, (x / 100) * H).toFixed(1), fill: x < 0 ? "var(--line-2)" : night ? (x >= 80 ? "#3d5afe" : "#8c9eff") : x >= 80 ? "#f2a900" : "#ffd36b" })));
}

function skyCard(s) {
  if (!s) return null;
  const tonight = s.nights?.[s.nights.length - 1], today = s.days?.[s.days.length - 1];
  return card("Its sky", s.t ? `GOES's cloud mask, last scan ${ago((Date.now() - Date.parse(s.t)) / 1000)}` : "",
    h("div", { class: "ea-row" }, h("span", {}, "Now"), h("span", {}, skyPill({ words: s.clear < 0 ? "not seen" : s.smoke >= 0.3 ? "smoke" : s.clear >= 0.8 ? "clear" : s.clear >= 0.4 ? "partly cloudy" : "cloudy" }), " ", h("span", { class: "ea-note" }, s.clear >= 0 ? `${fmt(s.clear * 100)}% of its pixels clear` : ""))),
    h("div", { class: "ea-row" }, h("span", {}, "Nights, the last 30"), h("span", { class: "ea-skyrow" }, skyBars(s.nights || [], true), h("span", { class: "ea-note" }, s.clear_nights == null ? "none read" : `${fmt(s.good_nights)} clear, ${fmt(s.clear_nights)}% on average`))),
    h("div", { class: "ea-row" }, h("span", {}, "Days, the last 30"), h("span", { class: "ea-skyrow" }, skyBars(s.days || [], false), h("span", { class: "ea-note" }, s.clear_days == null ? "none read" : `${fmt(s.good_days)} clear, ${fmt(s.clear_days)}% on average`))),
    h("p", { class: "ea-note" }, `A night counts its scans with the sun 12° down or more, a day those with it 5° up; a clear one is ${80}% clear or more.${tonight >= 0 ? ` Tonight so far: ${tonight}%.` : today >= 0 ? ` Today so far: ${today}%.` : ""}`));
}

async function skyPage(app, page, region) {
  const R = REGION[region];
  if (!R) throw new Error(`no region ${region}`);
  const box = h("div", { class: "ea-map" });
  const side = h("div");
  put(page, h("div", { class: "ea-hero", style: { paddingTop: "8px" } },
    h("h2", {}, "Where is the sky ", h("em", {}, "clear?")),
    h("p", {}, `GOES's cloud mask over every place of ${R.title}, ${region === "chile" ? "every 10 minutes" : "every 5 minutes"}, and its last 30 days and nights: for a night under the stars, a photograph, a flight or a solar panel.`)),
    regionSwitch(app, "sky", region), h("div", { class: "ea-split wide" }, box, side));
  const map = await makeMap(box, { center: R.center, zoom: R.zoom, bounds: R.bounds });
  app.stops.push(() => map.remove());
  const tl = h("div", { class: "ea-maptl" });
  box.append(tl);
  const skyNote = await addSky(app, map, region);
  const legend = h("div", { class: "ea-legend" });
  box.append(legend);
  const s = { mode: "now", at: null, km: 50, want: "now", q: "", label: "" };
  let L = null;
  const drawMap = () => {
    if (!L) return;
    const v = s.mode === "now" ? L.clear : s.mode === "nights" ? L.nights : L.days;
    const sq = squares(L.lat, L.lon, HALF);
    const features = [];
    for (let i = 0; i < sq.length; i++) {
      let x = v[i];
      if (x == null) continue;
      const [la, lo2, dl] = sq[i];
      const smoke = s.mode === "now" && (L.smoke[i] ?? 0) >= 3;
      // Now: clouds white, clear see-through; 30 days: the clearer, the deeper blue (nights) or gold (days).
      const t = s.mode === "now" ? 1 - x / 10 : x / 100;
      if (s.mode === "now" && t < 0.15 && !smoke) continue;
      features.push({ type: "Feature", properties: { t, smoke: smoke ? 1 : 0 }, geometry: { type: "Polygon", coordinates: square(la, lo2, HALF, dl) } });
    }
    const paint = s.mode === "now"
      ? { "fill-color": ["case", ["==", ["get", "smoke"], 1], "#a0522d", "#ffffff"], "fill-opacity": ["case", ["==", ["get", "smoke"], 1], 0.55, ["*", 0.75, ["get", "t"]]] }
      : { "fill-color": ["interpolate", ["linear"], ["get", "t"], 0, "#2b2b2b", 0.5, s.mode === "nights" ? "#4a5fc1" : "#c99a2e", 1, s.mode === "nights" ? "#a9c1ff" : "#ffe08a"], "fill-opacity": 0.7 };
    if (map.getLayer("skyq")) map.removeLayer("skyq");
    if (map.getSource("skyq")) map.removeSource("skyq");
    map.addSource("skyq", { type: "geojson", data: { type: "FeatureCollection", features } });
    map.addLayer({ id: "skyq", type: "fill", source: "skyq", paint }, "labels");
    legend.replaceChildren(s.mode === "now"
      ? h("div", {}, h("b", {}, "Clouds now"), h("div", {}, h("span", { class: "sw" }, h("i", { style: { background: "#fff" } }), "cloudy"), h("span", { class: "sw" }, h("i", { style: { background: "#a0522d" } }), "smoke"), h("span", { class: "sw" }, "clear: the ground shows")), h("div", { class: "ea-note", style: { color: "#ddd" } }, L.t ? `GOES's clear sky mask of ${when(L.t, region)}` : "No scan read yet."))
      : h("div", {}, h("b", {}, s.mode === "nights" ? "Clear nights, the last 30" : "Clear days, the last 30"), h("div", { class: "bar", style: { background: `linear-gradient(90deg, #2b2b2b, ${s.mode === "nights" ? "#4a5fc1, #a9c1ff" : "#c99a2e, #ffe08a"})` } }), h("div", { class: "ends" }, h("span", {}, "never clear"), h("span", {}, "always"))));
  };
  const modeChips = () => [["now", "Clouds now"], ["nights", "Clear nights"], ["days", "Clear days"]].map(([k, t]) => h("button", { class: `ea-mapbtn${s.mode === k ? " on" : ""}`, onclick: () => { s.mode = k; drawTop(); drawMap(); } }, t));
  const drawTop = () => tl.replaceChildren(h("div", { class: "ea-mapttl" }, h("b", {}, R.title), ` · ${R.sat}'s cloud mask`), h("div", { style: { display: "flex", gap: "6px", flexWrap: "wrap" } }, modeChips()), skyNote);
  drawTop();
  const load = async (fresh) => {
    L = await app.api(`/v1/sky?region=${region}`, { fresh, keep: 60000 });
    drawMap();
  };
  await load(false);
  const again = setInterval(() => load(true).catch(() => {}), 120000);
  app.stops.push(() => clearInterval(again));

  // The finder.
  const out = h("div");
  const found = h("div", { class: "ea-list" });
  const where = h("div", { class: "ea-note" }, "Anywhere in ", R.title, ".");
  let t = 0;
  const placeIn = h("input", { type: "search", autocomplete: "off", placeholder: "Near a town, or 34.05, -118.2", "aria-label": "Near where", oninput: async (e) => {
    const q = e.target.value.trim(), mine = ++t;
    if (q.length < 2) return swap(found);
    const r = await app.api(`/v1/find?q=${encodeURIComponent(q)}`, { keep: 60000 });
    if (mine === t) swap(found, r.found.filter((x) => x.region === region || !REGION[x.region]).slice(0, 6).map((x) => h("div", { class: "info", "data-i": "1", onclick: () => { s.at = [x.lat, x.lon]; s.label = x.label; placeIn.value = ""; swap(found); run(); } }, h("span", { class: "t" }, x.label))));
  } });
  const chips = (list, key) => h("div", { class: "ea-chips", style: { margin: "4px 0" } }, list.map(([k, txt]) => h("button", { class: `ea-chip${s[key] === k ? " on" : ""}`, onclick: () => { s[key] = k; if (key === "want" && k !== "now") { s.mode = k; drawTop(); drawMap(); } run(); } }, txt)));
  const lookIn = h("input", { type: "search", placeholder: "Looks like (optional): mountains, a dark desert, a beach…", "aria-label": "What the place looks like" });
  const form = h("div");
  const drawForm = () => swap(form,
    h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); found.querySelector("[data-i]")?.click(); } }, placeIn,
      h("button", { type: "button", class: "btn", onclick: () => navigator.geolocation?.getCurrentPosition((p) => { s.at = [p.coords.latitude, p.coords.longitude]; s.label = "your location"; run(); }, (err) => swap(out, h("div", { class: "ea-err" }, `No location: ${err.message}`))) }, "Use my location")),
    found, where,
    s.at ? chips([[25, "25 km"], [50, "50 km"], [100, "100 km"], [200, "200 km"]], "km") : null,
    chips([["now", "Clear now"], ["nights", "Most clear nights"], ["days", "Most clear days"]], "want"),
    h("form", { class: "ea-search", style: { marginTop: "6px" }, onsubmit: (e) => { e.preventDefault(); s.q = lookIn.value.trim(); run(); } }, lookIn, h("button", { class: "btn sm" }, "Find")));
  let mark = null;
  const run = async () => {
    drawForm();
    swap(where, s.at ? [`Within ${s.km} km of `, h("b", {}, s.label || `${fmt(s.at[0], 3)}, ${fmt(s.at[1], 3)}`), " · ", h("a", { href: "#", onclick: (e) => { e.preventDefault(); s.at = null; s.label = ""; run(); } }, "anywhere")] : ["Anywhere in ", R.title, "."]);
    swap(out, h("p", { class: "ea-note" }, "Looking…"));
    const qs = new URLSearchParams({ region, want: s.want, km: s.km, q: s.q });
    if (s.at) { qs.set("lat", s.at[0].toFixed(4)); qs.set("lon", s.at[1].toFixed(4)); }
    try {
      const j = await app.api(`/v1/sky/find?${qs}`, { fresh: true });
      if (s.at) {
        mark?.remove();
        mark = new window.maplibregl.Marker({ color: "#2f6fe0" }).setLngLat([s.at[1], s.at[0]]).addTo(map);
        map.fitBounds(ringsBounds([{ lat: s.at[0], lon: s.at[1], km: s.km }]), { padding: 30, duration: 600 });
      }
      swap(out,
        h("p", { class: "ea-note" }, j.seen ? `${fmt(j.clear_now)} of ${fmt(j.seen)} places ${s.q ? "like that " : ""}${s.at ? "here " : ""}are clear now.` : "No sky read here yet."),
        j.places.length ? h("div", { class: "ea-list" }, j.places.map((p) => h("div", { class: "info", onclick: () => app.go("place", p.id) },
          h("span", { class: "t" }, [p.town, p.adm2].filter(Boolean).filter((x, i, a) => a.indexOf(x) === i).join(", ") || `${fmt(p.lat, 3)}, ${fmt(p.lon, 3)}`),
          h("span", { class: "v" }, skyPill(p.sky)),
          h("span", { class: "m ea-skyline" }, p.km != null ? `${fmt(p.km, 1)} km ${p.dir} · ` : "", `${cap(p.kind)}, ${fmt(p.elev)} m · `,
            s.want === "days" ? [skyBars(p.sky.days, false), ` ${fmt(p.sky.good_days)} clear days`] : [skyBars(p.sky.nights, true), ` ${fmt(p.sky.good_nights)} clear nights`])))) : h("p", { class: "ea-empty" }, "None found."));
    } catch (e) { swap(out, h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  put(side, card("Find a clear place", "", form, out),
    card("How it's read", "", h("p", { class: "ea-note" }, `GOES's clear sky mask (ABI L2 ACM) says for each 2 km pixel whether it is clear, probably clear, probably cloudy or cloudy; a place is the share of its pixels clear. Smoke is the aerosol product's (ADP), by day only. Nights count the scans with the sun 12° below the horizon or more.`)));
  run();
}

// --------------------------------------------------------- future climate

const signed = (x, d = 1) => (x == null ? "–" : `${x > 0 ? "+" : x < 0 ? "−" : "±"}${fmt(Math.abs(x), d)}`);
const FUTURE_MODES = [
  ["hot", "Hotter summers", "The hottest month's highs, 2071-2100 against 1981-2010", ["#fff3e0", "#fdb863", "#e66101", "#7f2704"], 0, 6, (x) => `${signed(x)} °C`],
  ["cold", "Milder winters", "The coldest month's lows, 2071-2100 against 1981-2010", ["#f7fbff", "#9ecae1", "#3182bd", "#08306b"], 0, 6, (x) => `${signed(x)} °C`],
  ["rain", "Rain", "The year's rain, 2071-2100 against 1981-2010", ["#8c510a", "#d8b365", "#f5f5f5", "#5ab4ac", "#01665e"], -40, 40, (x) => `${signed(x, 0)}%`],
  ["km", "How far its future is", "How far the place is from the one whose climate today is nearest its own in 2071-2100", ["#f2f0f7", "#9e9ac8", "#6a51a3", "#3f007d"], 0, 1500, (x) => `${fmt(x)} km`],
];

/// A row of now against 2071-2100: the value, the models' mean later and
/// their range.
function futureRow(label, now, later, lo, hi, unit, d = 1) {
  const diff = now != null && later != null ? later - now : null;
  return [h("dt", {}, label), h("dd", {}, `${fmt(now, d)} → `, h("b", {}, `${fmt(later, d)} ${unit}`), diff != null ? h("span", { class: "ea-note" }, ` (${unit === "mm" ? `${signed(now ? (100 * diff) / now : 0, 0)}%` : signed(diff, d)}${lo != null && hi != null ? `; the models ${fmt(lo, d)} to ${fmt(hi, d)}` : ""})`) : null)];
}

function climateTable(c) {
  return h("dl", { class: "ea-kv" },
    futureRow("Hottest month, highs", c.t_hot, c.t_hot_70, c.t_hot_70_lo, c.t_hot_70_hi, "°C"),
    futureRow("Coldest month, lows", c.t_cold, c.t_cold_70, c.t_cold_70_lo, c.t_cold_70_hi, "°C"),
    futureRow("The year's mean", c.t_mean, c.t_mean_70, null, null, "°C"),
    futureRow("Rain a year", c.rain, c.rain_70, c.rain_70_lo, c.rain_70_hi, "mm", 0),
    futureRow("Rain of the warmest 3 months", c.rain_summer, c.rain_summer_70, null, null, "mm", 0),
    h("dt", {}, "Climate"), h("dd", {}, c.kg === c.kg_70 || !c.kg_70 ? `${c.kg}, ${KOPPEN_WORDS[c.kg] || c.kg}, stays` : [`${c.kg} (${KOPPEN_WORDS[c.kg] || c.kg}) → `, h("b", {}, `${c.kg_70} (${KOPPEN_WORDS[c.kg_70] || c.kg_70})`), h("span", { class: "ea-note" }, `, ${c.kg_70_agree} of 5 models`)]));
}

function analogLine(c) {
  if (c.novel) return h("p", {}, h("b", {}, "No place of California or Chile has a climate close to its 2071-2100 one today."), h("span", { class: "ea-note" }, ` The nearest, ${c.analog_town || c.analog}, is ${fmt(c.analog_d, 1)} standard deviations off.`));
  return h("p", {}, "Its 2071-2100 climate is like ", h("a", { href: `#/future/${c.analog_region}/${encodeURIComponent(c.analog)}` }, h("b", {}, c.analog_town || c.analog)), `'s today${c.analog_region !== c.region ? `, in ${REGION[c.analog_region]?.title || c.analog_region}` : ""}, ${fmt(c.analog_km)} km away.`);
}

/// The place page's card.
function climateCard(app, c) {
  if (!c) return null;
  return card("Its climate in 2071-2100", "CHELSA 2.1, SSP3-7.0, five models",
    analogLine(c), climateTable(c),
    h("p", { class: "ea-note" }, h("a", { href: app.href("future", c.region, c.id) }, "Where its future climate is today, and where its climate goes →")));
}

function analogList(app, list, later) {
  if (!list?.length) return h("p", { class: "ea-empty" }, "None found.");
  return h("div", { class: "ea-list" }, list.map((p) => h("div", { class: "info", onclick: () => app.go("future", p.region, p.id) },
    h("span", { class: "t" }, [p.town, p.adm2].filter(Boolean).filter((x, i, a) => a.indexOf(x) === i).join(", ") || `${fmt(p.lat, 3)}, ${fmt(p.lon, 3)}`),
    h("span", { class: "v" }, h("span", { class: `ea-pill ${p.d <= 0.5 ? "good" : p.d <= 1.0 ? "warn" : "mute"}`, title: "distance between the climates, in standard deviations" }, `${fmt(p.d, 2)} σ`)),
    h("span", { class: "m" }, `${REGION[p.region]?.title || p.region}, ${fmt(p.km)} km ${p.dir} · ${later ? `${p.kg_70} in 2071-2100 · highs ${fmt(p.t_hot_70, 1)} °C, ${fmt(p.rain_70)} mm` : `${p.kg} today · highs ${fmt(p.t_hot, 1)} °C, ${fmt(p.rain)} mm`}`))));
}

async function futurePage(app, page, region, placeId) {
  const R = REGION[region];
  if (!R) throw new Error(`no region ${region}`);
  const box = h("div", { class: "ea-map" });
  const side = h("div");
  const top = h("p", {}, `What each place of ${R.title} will feel like in 2071-2100 if emissions keep rising (SSP3-7.0), from five climate models downscaled by CHELSA, and where that climate can be visited today.`);
  put(page, h("div", { class: "ea-hero", style: { paddingTop: "8px" } }, h("h2", {}, "Your place in ", h("em", {}, "2071-2100")), top),
    regionSwitch(app, "future", region), h("div", { class: "ea-split wide" }, box, side));
  const map = await makeMap(box, { center: R.center, zoom: R.zoom, bounds: R.bounds });
  app.stops.push(() => map.remove());
  const tl = h("div", { class: "ea-maptl" });
  const legend = h("div", { class: "ea-legend" });
  box.append(tl, legend);
  const s = { mode: "hot" };
  const L = await app.api(`/v1/future?region=${region}`, { keep: 3600000 });
  if (L.n) {
    const m = L.median;
    swap(top, `What each place of ${R.title} will feel like in 2071-2100 if emissions keep rising (SSP3-7.0), from five climate models downscaled by CHELSA, and where that climate can be visited today. Across its ${fmt(L.n)} places, the middle one's hottest month gets ${signed(m.hot)} °C, its coldest nights ${signed(m.cold)} °C and its rain ${signed(m.rain, 0)}%; ${fmt((100 * L.class_changed) / L.n)}% change climate class${L.novel_n ? `, and ${fmt(L.novel_n)} get a climate no place of the two regions has anything close to today` : ""}.`);
  }
  const drawMap = () => {
    if (!L.n) return;
    const [k, title, about, colors, lo0, hi0, show] = FUTURE_MODES.find((x) => x[0] === s.mode);
    const v = L[k];
    // The scale spans the region's own spread (its 2nd to 98th centile), so
    // a change that is everywhere still shows where it is most.
    const scale = k === "hot" || k === "cold" ? 10 : 1;
    const sorted = v.filter((x) => x != null).map((x) => x / scale).sort((a, b) => a - b);
    const q = (f) => sorted[Math.min(sorted.length - 1, Math.floor(f * sorted.length))];
    let [lo, hi] = k === "km" ? [lo0, Math.min(hi0, q(0.98))] : [q(0.02), q(0.98)];
    if (k === "rain") { const m = Math.max(Math.abs(lo), Math.abs(hi)); [lo, hi] = [-m, m]; }
    if (!(hi > lo)) [lo, hi] = [lo0, hi0];
    const sq = squares(L.lat, L.lon, HALF);
    const features = [];
    for (let i = 0; i < sq.length; i++) {
      if (v[i] == null) continue;
      const x = v[i] / scale;
      const [la, lo2, dl] = sq[i];
      features.push({ type: "Feature", properties: { t: Math.max(0, Math.min(1, (x - lo) / (hi - lo))), novel: L.novel[i] }, geometry: { type: "Polygon", coordinates: square(la, lo2, HALF, dl) } });
    }
    const stops = colors.flatMap((c, i) => [i / (colors.length - 1), c]);
    setData(map, "futq", { type: "FeatureCollection", features }, [
      { id: "futq", type: "fill", before: "labels", paint: { "fill-color": ["interpolate", ["linear"], ["get", "t"], ...stops], "fill-opacity": 0.72 } },
      { id: "futn", type: "line", before: "labels", filter: ["==", ["get", "novel"], 1], paint: { "line-color": "#d7191c", "line-width": 0.6, "line-opacity": 0.8 } },
    ]);
    // The layers are made once: each mode sets its own colours.
    map.setPaintProperty("futq", "fill-color", ["interpolate", ["linear"], ["get", "t"], ...stops]);
    map.setLayoutProperty("futn", "visibility", k === "km" ? "visible" : "none");
    legend.replaceChildren(h("b", {}, title), h("div", { style: { opacity: 0.85 } }, about), h("div", { class: "bar", style: { background: `linear-gradient(90deg, ${colors.join(",")})` } }), h("div", { class: "ends" }, h("span", {}, show(lo)), h("span", {}, show(hi))),
      ...(k === "km" ? [h("div", { class: "ea-note", style: { color: "#ddd" } }, h("span", { style: { color: "#ff6b6b" } }, "Red edges"), ": no place has a climate close to it today")] : []));
  };
  const drawTop = () => tl.replaceChildren(h("div", { class: "ea-mapttl" }, h("b", {}, R.title), " · 2071-2100 against 1981-2010"),
    h("div", { style: { display: "flex", gap: "6px", flexWrap: "wrap" } }, FUTURE_MODES.map(([k, t]) => h("button", { class: `ea-mapbtn${s.mode === k ? " on" : ""}`, onclick: () => { s.mode = k; drawTop(); drawMap(); } }, t))));
  drawTop();
  drawMap();
  if (!L.n) swap(side, card("Not read yet", "", h("p", { class: "ea-note" }, "This region's climate has not been loaded (tools/climate.py).")));

  // A place: from the finder, a click or the address.
  const found = h("div", { class: "ea-list" });
  let t = 0;
  const placeIn = h("input", { type: "search", autocomplete: "off", placeholder: "A town, or 34.05, -118.2", "aria-label": "A place", oninput: async (e) => {
    const q = e.target.value.trim(), mine = ++t;
    if (q.length < 2) return swap(found);
    const r = await app.api(`/v1/find?q=${encodeURIComponent(q)}`, { keep: 60000 });
    if (mine === t) swap(found, r.found.filter((x) => REGION[x.region]).slice(0, 6).map((x) => h("div", { class: "info", "data-i": "1", onclick: () => pickAt(x.lat, x.lon) }, h("span", { class: "t" }, x.label))));
  } });
  const finder = h("div", {},
    h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); found.querySelector("[data-i]")?.click(); } }, placeIn,
      h("button", { type: "button", class: "btn", onclick: () => navigator.geolocation?.getCurrentPosition((p) => pickAt(p.coords.latitude, p.coords.longitude), (err) => swap(out, h("div", { class: "ea-err" }, `No location: ${err.message}`))) }, "Use my location")),
    found, h("p", { class: "ea-note" }, "Or click the map."));
  const out = h("div");
  put(side, card("Pick a place", "", finder), out);
  let scope = "all";
  const pickAt = async (lat, lon) => {
    const at = await app.api(`/v1/at?lat=${lat.toFixed(5)}&lon=${lon.toFixed(5)}`);
    if (at.id) app.go("future", at.region, at.id);
    else swap(out, h("p", { class: "ea-empty" }, "No place there: Earth has California and Chile."));
  };
  map.on("click", (e) => pickAt(e.lngLat.lat, e.lngLat.lng).catch((err) => swap(out, h("div", { class: "ea-err" }, String(err.message || err)))));
  const showPlace = async () => {
    swap(found); placeIn.value = "";
    swap(out, h("p", { class: "ea-note" }, "Looking…"));
    try {
      const j = await app.api(`/v1/future/place/${encodeURIComponent(placeId)}?scope=${scope}`);
      const c = j.climate;
      const dl = HALF / Math.cos((c.lat * Math.PI) / 180);
      const lines = j.like_later.slice(0, 5).map((p) => ({ type: "Feature", properties: {}, geometry: { type: "LineString", coordinates: [[c.lon, c.lat], [p.lon, p.lat]] } }));
      setData(map, "futsel", { type: "FeatureCollection", features: [{ type: "Feature", properties: {}, geometry: { type: "Polygon", coordinates: square(c.lat, c.lon, HALF, dl) } }, ...lines] },
        [{ id: "futsel", type: "line", paint: { "line-color": "#fff", "line-width": 2.2, "line-dasharray": [2, 1.5] } }]);
      if (c.region === region && !j.like_later.some((p) => p.region !== region)) {
        const pts = [c, ...j.like_later.slice(0, 5)];
        map.fitBounds([[Math.min(...pts.map((p) => p.lon)) - 0.3, Math.min(...pts.map((p) => p.lat)) - 0.3], [Math.max(...pts.map((p) => p.lon)) + 0.3, Math.max(...pts.map((p) => p.lat)) + 0.3]], { padding: 40, duration: 600, maxZoom: 8 });
      }
      const scopes = h("div", { class: "ea-chips", style: { margin: "4px 0" } }, [["all", "California and Chile"], ["region", `Only ${REGION[c.region].title}`]].map(([k, txt]) => h("button", { class: `ea-chip${scope === k ? " on" : ""}`, onclick: () => { scope = k; showPlace(); } }, txt)));
      swap(out,
        card(c.town ? `${c.town}` : `${fmt(c.lat, 3)}, ${fmt(c.lon, 3)}`, `${REGION[c.region].title} · ${fmt(c.lat, 3)}, ${fmt(c.lon, 3)}`,
          analogLine(c), climateTable(c), h("p", { class: "ea-note" }, h("a", { href: app.href("place", c.id) }, "Its page →"))),
        card("Where its 2071-2100 climate is today", "the nearest climates of 1981-2010, one place a town", scopes, analogList(app, j.like_later, false)),
        card("Where its climate today will be in 2071-2100", "the places whose 2071-2100 climate is nearest its 1981-2010 one", analogList(app, j.goes_to, true)));
    } catch (e) { swap(out, h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  if (placeId) showPlace();
  else if (L.n) swap(out, card("How it's worked out", "",
    h("p", { class: "ea-note" }, "CHELSA 2.1 gives the climate of every square kilometre for 1981-2010 and, from five CMIP6 models (GFDL-ESM4, IPSL-CM6A-LR, MPI-ESM1-2-HR, MRI-ESM2-0 and UKESM1-0-LL), for 2071-2100. Each place's climate is six numbers: the year's mean temperature, the hottest month's highs, the coldest month's lows, the year's rain, how unevenly it falls and the summer's rain. Standardised, they are a vector, once for now and once for later, and the place whose vector today is nearest a place's later one is where its future can be visited now (ChloeDB's distance() over 103,664 vectors).")));
}

// ------------------------------------------------------------- land finder

const kmText = (x) => (x == null ? "–" : x < 1 ? `${fmt(Math.round(x * 100) * 10)} m` : `${fmt(x, 1)} km`);
const kvText = (x) => (x ? `${fmt(x)} kV` : "voltage not given");

/// The place page's card: what is around it for building.
function landCard(app, p) {
  if (p.sub_km == null && p.road_km == null) return null;
  return card("For building on it", "OpenStreetMap, PAD-US",
    h("dl", { class: "ea-kv" },
      h("dt", {}, "Substation"), h("dd", {}, `${kmText(p.sub_km)} · ${kvText(p.sub_kv)}${p.sub_name ? ` · ${p.sub_name}` : ""}`),
      h("dt", {}, "Power line"), h("dd", {}, `${kmText(p.line_km)} · ${kvText(p.line_kv)}`),
      h("dt", {}, "Main road"), h("dd", {}, `${kmText(p.road_km)}${p.road_name ? ` · ${p.road_name}` : ""}`),
      h("dt", {}, "Solar farms"), h("dd", {}, p.solar > 0 ? `${pct(p.solar * 100)} of it${p.solar_name ? `, ${p.solar_name}` : ""}` : p.solar_km != null ? `nearest ${kmText(p.solar_km)}${p.solar_name ? `, ${p.solar_name}` : ""}` : "none known"),
      h("dt", {}, "Protected"), h("dd", {}, p.protected > 0 ? `${pct(p.protected * 100)} of it${p.protected_strict > 0 ? ` (${pct(p.protected_strict * 100)} strictly)` : ""}${p.protected_name ? `, ${p.protected_name}` : ""}` : "none"),
      h("dt", {}, "Slope"), h("dd", {}, `${fmt(p.slope, 1)}° median`)),
    h("p", { class: "ea-note" }, h("a", { href: app.href("land", p.region) }, "Find land like this →")));
}

async function landPage(app, page, region) {
  const R = REGION[region];
  if (!R) throw new Error(`no region ${region}`);
  const box = h("div", { class: "ea-map" });
  const side = h("div");
  put(page, h("div", { class: "ea-hero", style: { paddingTop: "8px" } },
    h("h2", {}, "Land ", h("em", {}, "finder")),
    h("p", {}, `Places of ${R.title} to build a solar farm on: flat, near a substation and a road, not protected and not a town, ranked by how much they look like the solar farms already built here. One statement keeps the places within the limits and ranks them by their Sentinel-2 picture.`)),
    regionSwitch(app, "land", region), h("div", { class: "ea-split wide" }, box, side));
  const map = await makeMap(box, { center: R.center, zoom: R.zoom, bounds: R.bounds });
  app.stops.push(() => map.remove());
  const legend = h("div", { class: "ea-legend" }, h("div", {}, h("span", { class: "sw" }, h("i", { style: { background: "#ffd400" } }), "solar farms built"), " ", h("span", { class: "sw" }, h("i", { style: { background: "#2f6fe0" } }), "places found")));
  box.append(legend);
  const s = store.get("ea-land", null) || {};
  const ask = { slope: s.slope ?? 5, sub: s.sub ?? 10, road: s.road ?? 5, protected: s.protected || "none", built: s.built ?? 10, kv: s.kv ?? 0, rank: s.rank || "solar", q: "", at: null, km: 50, label: "" };
  let shortlist = store.get("ea-shortlist", []);
  const out = h("div");
  const form = h("div");
  const listBox = h("div");
  const chips = (key, list) => h("div", { class: "ea-chips", style: { margin: "2px 0 6px" } }, list.map(([v, t]) => h("button", { class: `ea-chip${ask[key] === v ? " on" : ""}`, onclick: () => { ask[key] = v; run(); } }, t)));
  const row = (label, el) => h("div", {}, h("div", { class: "ea-note", style: { marginTop: "6px" } }, label), el);
  const lookIn = h("input", { type: "search", placeholder: "Looks like: flat dry grassland, old orchards…", "aria-label": "What the land looks like" });
  const found = h("div", { class: "ea-list" });
  let t = 0;
  const nearIn = h("input", { type: "search", autocomplete: "off", placeholder: "Near a town (optional)", "aria-label": "Near where", oninput: async (e) => {
    const q = e.target.value.trim(), mine = ++t;
    if (q.length < 2) return swap(found);
    const r = await app.api(`/v1/find?q=${encodeURIComponent(q)}`, { keep: 60000 });
    if (mine === t) swap(found, r.found.filter((x) => x.region === region).slice(0, 6).map((x) => h("div", { class: "info", "data-i": "1", onclick: () => { ask.at = [x.lat, x.lon]; ask.label = x.label; nearIn.value = ""; swap(found); run(); } }, h("span", { class: "t" }, x.label))));
  } });
  const drawForm = () => swap(form,
    row("Slope, at most", chips("slope", [[2, "2°"], [5, "5°"], [10, "10°"], [null, "any"]])),
    row("A substation within", chips("sub", [[2, "2 km"], [5, "5 km"], [10, "10 km"], [25, "25 km"], [null, "any"]])),
    row("Of at least", chips("kv", [[0, "any voltage"], [69, "69 kV"], [115, "115 kV"], [220, "220 kV"]])),
    row("A main road within", chips("road", [[2, "2 km"], [5, "5 km"], [10, "10 km"], [null, "any"]])),
    row("Protected land", chips("protected", [["none", "leave it all out"], ["strict", "leave out strict only"], ["any", "keep it"]])),
    row("Towns", chips("built", [[2, "no buildings"], [10, "under 10% built"], [null, "keep them"]])),
    row("Rank by", chips("rank", [["solar", "like its solar farms"], ["grid", "nearest the grid"], ["words", "like words"]])),
    ask.rank === "words" ? h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); ask.q = lookIn.value.trim(); run(); } }, lookIn, h("button", { class: "btn sm" }, "Find")) : null,
    h("form", { class: "ea-search", style: { marginTop: "8px" }, onsubmit: (e) => { e.preventDefault(); found.querySelector("[data-i]")?.click(); } }, nearIn,
      ask.at ? h("button", { type: "button", class: "btn sm", onclick: () => { ask.at = null; ask.label = ""; run(); } }, `Not only near ${ask.label}`) : null),
    found,
    ask.at ? row(`Within, of ${ask.label}`, chips("km", [[25, "25 km"], [50, "50 km"], [100, "100 km"], [200, "200 km"]])) : null);
  const inList = (id) => shortlist.some((x) => x.id === id);
  const toggle = (p) => {
    shortlist = inList(p.id) ? shortlist.filter((x) => x.id !== p.id) : [...shortlist, { id: p.id, region: p.region, name: placeName(p), lat: p.lat, lon: p.lon, sub_km: p.sub_km, slope: p.slope }];
    store.set("ea-shortlist", shortlist);
    drawList();
  };
  const csv = () => {
    const lines = [["id", "name", "lat", "lon", "km to substation", "slope"].join(","), ...shortlist.map((x) => [x.id, `"${String(x.name).replace(/"/g, '""')}"`, x.lat, x.lon, x.sub_km, x.slope].join(","))];
    const a = h("a", { href: URL.createObjectURL(new Blob([lines.join("\n")], { type: "text/csv" })), download: "shortlist.csv" });
    a.click();
  };
  const watch = async (x) => {
    try {
      const r = await app.api(`/v1/watches?${wq()}`, { body: { name: x.name, lat: x.lat, lon: x.lon, km: 10 } });
      app.go("watch", r.watch.id);
    } catch (e) { swap(listBox, h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  const drawList = () => swap(listBox, card("Shortlist", "kept in this browser",
    shortlist.length ? [h("div", { class: "ea-list" }, shortlist.map((x) => h("div", { class: "info" },
      h("a", { class: "t", href: app.href("place", x.id) }, x.name),
      h("span", { class: "v" }, h("button", { class: "btn sm", title: "Be told of a fire within 10 km", onclick: () => watch(x) }, "Watch"), " ", h("button", { class: "btn sm", onclick: () => toggle(x) }, "Remove")),
      h("span", { class: "m" }, `${kmText(x.sub_km)} to a substation · slope ${fmt(x.slope, 1)}°`)))),
    h("p", {}, h("button", { class: "btn sm", onclick: csv }, "Download as CSV"))] : h("p", { class: "ea-note" }, "Places you star are kept here, with a fire watch a click away.")));
  const run = async () => {
    store.set("ea-land", { slope: ask.slope, sub: ask.sub, road: ask.road, protected: ask.protected, built: ask.built, kv: ask.kv, rank: ask.rank === "words" ? "solar" : ask.rank });
    drawForm();
    if (ask.rank === "words" && !ask.q) return swap(out, h("p", { class: "ea-note" }, "Say what the land should look like."));
    swap(out, h("p", { class: "ea-note" }, "Looking…"));
    const qs = new URLSearchParams({ region, protected: ask.protected, rank: ask.rank, q: ask.q });
    for (const k of ["slope", "sub", "road", "built", "kv"]) if (ask[k] != null && ask[k] !== 0) qs.set(k, ask[k]);
    if (ask.at) { qs.set("lat", ask.at[0].toFixed(4)); qs.set("lon", ask.at[1].toFixed(4)); qs.set("km", ask.km); }
    try {
      const j = await app.api(`/v1/land?${qs}`, { fresh: true });
      const sqf = (list, color) => list.map((p) => ({ type: "Feature", properties: { c: color }, geometry: { type: "Polygon", coordinates: square(p.lat, p.lon, HALF, HALF / Math.cos((p.lat * Math.PI) / 180)) } }));
      setData(map, "land", { type: "FeatureCollection", features: [...sqf(j.farms, "#ffd400"), ...sqf(j.places, "#2f6fe0")] },
        [{ id: "land", type: "fill", before: "labels", paint: { "fill-color": ["get", "c"], "fill-opacity": 0.75 } }, { id: "landl", type: "line", before: "labels", paint: { "line-color": ["get", "c"], "line-width": 1.5 } }]);
      if (j.places.length) {
        const la = j.places.map((p) => p.lat), lo = j.places.map((p) => p.lon);
        map.fitBounds([[Math.min(...lo) - 0.2, Math.min(...la) - 0.2], [Math.max(...lo) + 0.2, Math.max(...la) + 0.2]], { padding: 40, duration: 600, maxZoom: 9 });
      }
      swap(out,
        h("p", { class: "ea-note" }, `${fmt(j.within)}${j.more ? " or more" : ""} places are within these limits${ask.at ? ` near ${ask.label}` : ""}; the best ${fmt(j.places.length)}${j.rank === "solar" ? `, by how much they look like the ${fmt(j.farms.length)} places with a solar farm built` : j.rank === "words" ? `, by how much they look like “${ask.q}”` : ", nearest a substation first"}:`),
        j.places.length ? h("div", { class: "ea-list" }, j.places.map((p) => h("div", { class: "info" },
          h("a", { class: "t", href: app.href("place", p.id) }, placeName(p)),
          h("span", { class: "v" }, h("button", { class: `btn sm${inList(p.id) ? " on" : ""}`, title: "Keep it on the shortlist", onclick: (e) => { toggle(p); e.target.classList.toggle("on"); e.target.textContent = inList(p.id) ? "★" : "☆"; } }, inList(p.id) ? "★" : "☆")),
          h("span", { class: "m" }, `${p.km != null ? `${fmt(p.km)} km ${p.dir} · ` : ""}${cap(p.kind)}, slope ${fmt(p.slope, 1)}° · substation ${kmText(p.sub_km)}${p.sub_kv ? ` (${fmt(p.sub_kv)} kV)` : ""} · road ${kmText(p.road_km)}${p.protected > 0 ? ` · ${pct(p.protected * 100)} protected` : ""}`)))) : h("p", { class: "ea-empty" }, "None: loosen a limit."));
    } catch (e) { swap(out, h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  put(side, card("Limits", "", form), card("Places", "", out), listBox,
    card("Where it comes from", "", h("p", { class: "ea-note" }, "Power lines, substations, main roads and solar farms are OpenStreetMap's (© OpenStreetMap contributors, ODbL); a solar farm is a hectare or more. Protected land is PAD-US 4.1 (USGS) in California and OpenStreetMap's in Chile; strict is GAP 1 and 2, or a national park or reserve. Distances are to the place's centre.")));
  drawList();
  run();
}

// ------------------------------------------------------------------ search

const EXAMPLES = ["vineyards on hills", "a lithium mine with evaporation ponds", "a burn scar in a forest", "almond orchards", "a solar farm", "a glacier", "a salt flat", "suburbs next to a forest", "an open pit copper mine", "pine plantations"];

async function search(app, page, q) {
  const params = new URLSearchParams(location.hash.split("?")[1] || "");
  const region = params.get("region") || "";
  const mode = params.get("mode") || "both";
  const form = h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); const v = e.target.q.value.trim(); if (v) location.hash = `${app.href("search", v)}?region=${region}&mode=${mode}`; } },
    h("input", { type: "search", name: "q", value: q, placeholder: "What a place looks like: vineyards on hills, a lithium mine, a burn scar…" }),
    h("button", { class: "btn primary" }, "Search"), h("a", { class: "btn", href: app.href("picture") }, "By a picture"));
  const link = (r, m) => `${app.href("search", q)}?region=${r}&mode=${m}`;
  put(page, form, h("div", { class: "ea-chips" },
    [["", "Both regions"], ["california", "California"], ["chile", "Chile"]].map(([k, t]) => h("a", { class: `ea-chip${k === region ? " on" : ""}`, href: link(k, mode) }, t)),
    h("span", { style: { width: "12px" } }),
    [["both", "Picture and what covers it"], ["look", "The picture only"], ["words", "The description, with names"]].map(([k, t]) => h("a", { class: `ea-chip${k === mode ? " on" : ""}`, href: link(region, k), title: k === "look" ? "SigLIP: the words' vector against each place's picture" : k === "words" ? "BM25 over each place's description, with its town and county" : "the picture's rank (SigLIP) and the land cover's (BM25 over what covers it), fused by rrf" }, t))));
  if (!q) {
    put(page, card("Try", "", h("div", { class: "ea-chips" }, EXAMPLES.map((x) => h("a", { class: "ea-chip", href: app.href("search", x) }, x)))));
    return;
  }
  const j = await app.api(`/v1/search?q=${encodeURIComponent(q)}&mode=${mode}${region ? `&region=${region}` : ""}`);
  await results(app, page, j.places, `${fmt(j.places.length)} places that look like “${j.words}”`, `the words read as “${j.prompt}” by SigLIP so400m`);
}

async function results(app, page, places, title, sub) {
  const box = h("div", { class: "ea-map short" });
  const grid = h("div", { class: "ea-grid" }, places.map((p, i) => tile(app, p, `#${i + 1}${p.score != null ? ` · ${fmt(p.score, 3)}` : ""}`)));
  put(page, h("div", { class: "ea-split wide" }, card(title, sub, grid), box));
  if (!places.length) return;
  const map = await makeMap(box, { center: [places[0].lon, places[0].lat], zoom: 5 });
  app.stops.push(() => map.remove());
  const fc = { type: "FeatureCollection", features: places.map((p, i) => ({ type: "Feature", properties: { id: p.id, r: i }, geometry: { type: "Point", coordinates: [p.lon, p.lat] } })) };
  setData(map, "hits", fc, [{ id: "hits", type: "circle", paint: { "circle-radius": ["interpolate", ["linear"], ["get", "r"], 0, 8, 60, 4], "circle-color": "#ffcc33", "circle-stroke-color": "#222", "circle-stroke-width": 1 } }]);
  const b = new window.maplibregl.LngLatBounds();
  places.forEach((p) => b.extend([p.lon, p.lat]));
  map.fitBounds(b, { padding: 50, maxZoom: 9, duration: 0 });
  map.on("click", "hits", (e) => { const id = e.features?.[0]?.properties?.id; if (id) app.go("place", id); });
}

async function picture(app, page) {
  const out = h("div");
  const input = h("input", { type: "file", accept: "image/*", style: { display: "none" } });
  const drop = h("div", { class: "ea-drop" }, h("b", {}, "Drop a picture here, paste one, or click to choose"), h("div", { class: "ea-note" }, "A satellite or aerial picture works best: a field, a mine, a coast. Sent to this app's server and searched; not kept."));
  let region = "";
  const regions = h("div", { class: "ea-chips" }, [["", "Both regions"], ["california", "California"], ["chile", "Chile"]].map(([k, t]) => h("button", { class: `ea-chip${k === region ? " on" : ""}`, onclick: (e) => { region = k; [...regions.children].forEach((c) => c.classList.toggle("on", c === e.target)); } }, t)));
  const send = async (file) => {
    if (!file) return;
    if (file.size > 5 << 20) { out.replaceChildren(h("div", { class: "ea-err" }, "Pictures of at most 5 MB.")); return; }
    const url = await new Promise((ok) => { const r = new FileReader(); r.onload = () => ok(r.result); r.readAsDataURL(file); });
    drop.replaceChildren(h("img", { src: url, alt: "your picture" }), h("div", { class: "ea-note" }, "Searching…"));
    try {
      const j = await app.api("/v1/picture", { body: { image: url, region: region || undefined } });
      drop.replaceChildren(h("img", { src: url, alt: "your picture" }), h("div", { class: "ea-note" }, "Drop another, or click"));
      out.replaceChildren();
      await results(app, out, j.places, `${fmt(j.places.length)} places that look like your picture`, "its SigLIP vector against each place's");
    } catch (e) { out.replaceChildren(h("div", { class: "ea-err" }, String(e.message || e))); }
  };
  drop.onclick = () => input.click();
  input.onchange = () => send(input.files[0]);
  drop.ondragover = (e) => { e.preventDefault(); drop.classList.add("over"); };
  drop.ondragleave = () => drop.classList.remove("over");
  drop.ondrop = (e) => { e.preventDefault(); drop.classList.remove("over"); send(e.dataTransfer.files[0]); };
  const paste = (e) => { const f = [...(e.clipboardData?.files || [])][0]; if (f) send(f); };
  addEventListener("paste", paste);
  app.stops.push(() => removeEventListener("paste", paste));
  put(page, card("Search by a picture", "", regions, drop, input), out);
}

// ------------------------------------------------------------------ twins

function pair(app, a, b, sim) {
  return h("div", { class: "ea-pair" },
    h("div", { class: "two" },
      h("img", { loading: "lazy", src: thumbUrl(a.lat, a.lon), alt: placeName(a), onclick: () => app.go("place", a.id) }),
      b ? h("img", { loading: "lazy", src: thumbUrl(b.lat, b.lon), alt: placeName(b), onclick: () => app.go("place", b.id) }) : h("div")),
    h("div", { class: "lbl" },
      h("div", {}, h("b", {}, placeName(a)), h("span", {}, [REGION[a.region]?.title, cap(a.kind)].filter(Boolean).join(" · "))),
      b ? h("div", {}, h("b", {}, placeName(b)), h("span", {}, [REGION[b.region]?.title, cap(b.kind)].filter(Boolean).join(" · "))) : h("div")),
    h("div", { class: "sim" }, a.climate ? h("span", { class: "ea-chip", style: { cursor: "default" } }, h("i", { style: { background: KOPPEN[a.climate] || "#999" } }), `${a.climate} ${KOPPEN_WORDS[a.climate] || ""}`) : null, `similarity ${fmt(sim, 3)}`));
}

async function twins(app, page, from, climate) {
  const params = new URLSearchParams(location.hash.split("?")[1] || "");
  const kind = params.get("kind") || "";
  const j = await app.api(`/v1/twins?from=${from}${climate ? `&climate=${encodeURIComponent(climate)}` : ""}${kind ? `&kind=${encodeURIComponent(kind)}` : ""}`);
  const mine = j.climates.filter((c) => c.region === from && c.climate);
  const theirs = new Map(j.climates.filter((c) => c.region === j.to).map((c) => [c.climate, c.places]));
  const kinds = ["forest", "shrubland", "grassland", "farmland", "city", "bare", "water", "snow and ice", "mine"];
  const kl = (k) => `${app.href("twins", from, climate)}${k ? `?kind=${encodeURIComponent(k)}` : ""}`;
  put(page, 
    h("p", { class: "ea-sentence", style: { maxWidth: "820px", margin: "0 0 10px" } }, "California and central Chile are among the five places on Earth with a Mediterranean climate, on opposite sides of the equator. Here each place is paired with the place of the other whose picture looks most like it, among those with the same climate (Köppen-Geiger, 1991-2020)."),
    h("div", { class: "ea-chips" }, Object.entries(REGION).map(([k, r]) => h("a", { class: `ea-chip${k === from ? " on" : ""}`, href: app.href("twins", k, climate) }, `From ${r.title}`))),
    h("div", { class: "ea-chips" }, h("a", { class: `ea-chip${climate ? "" : " on"}`, href: app.href("twins", from) }, "Every climate"),
      mine.filter((c) => theirs.has(c.climate)).slice(0, 14).map((c) => h("a", { class: `ea-chip${c.climate === climate ? " on" : ""}`, href: app.href("twins", from, c.climate), title: KOPPEN_WORDS[c.climate] || "" }, h("i", { style: { background: KOPPEN[c.climate] || "#999" } }), c.climate, h("span", { class: "c" }, `${fmt(c.places)} · ${fmt(theirs.get(c.climate))}`)))),
    h("div", { class: "ea-chips" }, h("a", { class: `ea-chip${kind ? "" : " on"}`, href: kl("") }, "Any land"), kinds.map((k) => h("a", { class: `ea-chip${k === kind ? " on" : ""}`, href: kl(k) }, cap(k)))),
    j.pairs.length ? h("div", { class: "ea-pairs" }, j.pairs.map((p) => pair(app, p.a, p.b, p.sim))) : h("p", { class: "ea-empty" }, "No twins here."));
}

// ------------------------------------------------------------------ changed

const chipCache = new Map();
/// A place's picture of a year: a plain URL on the app's own page, a blob
/// read through the key in the command center.
async function chipSrc(app, place, year) {
  return mediaSrc(app, `/v1/chip/${encodeURIComponent(place)}/${year}`);
}
/// A picture the app serves: its path where the page can load it as it is,
/// else (in the command center, whose requests carry the key in a header)
/// read as base64 and made a blob.
async function mediaSrc(app, path) {
  if (app.ctx.media) return app.ctx.media(path);
  if (chipCache.has(path)) return chipCache.get(path);
  const p = app.ctx.api(`${path}?b64=1`, { timeout: 30000 }).then((j) => {
    const bin = atob(j.data);
    const a = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
    return URL.createObjectURL(new Blob([a], { type: j.mime }));
  });
  chipCache.set(path, p);
  return p;
}
function chip(app, place, year, label, big) {
  const img = h("img", { alt: `${year}`, loading: "lazy" });
  chipSrc(app, place, year).then((u) => { img.src = u; }).catch(() => { img.alt = "no picture"; });
  return h("div", { class: "ea-tile", style: { cursor: "default" } }, img, h("div", { class: "c" }, h("b", {}, String(year)), label ? h("span", {}, label) : null));
}

async function changed(app, page, q) {
  const form = h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); app.go("changed", e.target.q.value.trim()); } },
    h("input", { type: "search", name: "q", value: q, placeholder: "a lake became a dry lake bed, forest became a burn scar…" }), h("button", { class: "btn primary" }, "Find"));
  put(page, h("p", { class: "ea-sentence", style: { maxWidth: "820px", margin: "0 0 10px" } }, "Each place's look in the clearest Sentinel-2 scene of every other dry season since 2017, embedded by the same SigLIP model as the search. Ask for what a place was and what it became: ChloeDB ranks by the first year's look against the first words plus the last year's against the last."), form);
  const j = await app.api(`/v1/changed${q ? `?q=${encodeURIComponent(q)}` : ""}`);
  put(page, h("div", { class: "ea-chips" }, ["a lake became a dry lake bed", "a forest became a burn scar", "green fields became a solar farm", "shrubland became suburbs", "a desert became evaporation ponds", "a forest became a clear cut forest"].map((x) => h("a", { class: `ea-chip${x === q ? " on" : ""}`, href: app.href("changed", x) }, x))));
  if (!j.places.length) {
    put(page, card("Changed places", "", h("p", { class: "ea-note" }, "No years loaded yet.")));
    return;
  }
  put(page, h("h3", { style: { margin: "6px 0 10px", fontSize: "15px" } }, cap(j.what), h("span", { class: "ea-note" }, `  ${fmt(j.places.length)} places`)));
  const list = h("div", { class: "ea-pairs" });
  for (const c of j.places.slice(0, 24)) {
    list.append(h("div", { class: "ea-pair" },
      h("div", { class: "two" }, chip(app, c.id, c.first, c.from_label), chip(app, c.id, c.last, c.to_label)),
      h("div", { class: "lbl", style: { gridTemplateColumns: "1fr" } }, h("div", {}, h("b", {}, `${c.town || c.adm1 || c.id}${c.adm1 && c.town ? `, ${c.adm1}` : ""}`), h("span", {}, `${REGION[c.region]?.title || ""} · ${c.arc || `${c.from_label} → ${c.to_label}`}`))),
      h("div", { class: "sim" }, `moved most in ${c.step_year}`, h("a", { href: app.href("place", c.id), style: { marginLeft: "auto" } }, "Every year →"))));
  }
  put(page, list);
}

// -------------------------------------------------------------- rules, data

async function rules(app, page) {
  const j = await app.api("/v1/rules");
  put(page, card("Alert rules", "each a SQL statement over the fires, run every minute; its rows are its alerts",
    h("table", {}, h("thead", {}, h("tr", {}, h("th", {}, "Rule"), h("th", {}, "SQL"), h("th", { class: "num" }, "Last run"))),
      h("tbody", {}, j.rules.map((r) => h("tr", {}, h("td", {}, h("b", {}, r.title), h("div", { class: "ea-note" }, r.about)), h("td", {}, h("code", {}, r.sql)), h("td", { class: "num" }, r.last_run ? `${fmt(r.last_rows)} rows, ${fmt(r.last_ms, 1)} ms` : "not yet"))))))),
  card("Open alerts", `${fmt(j.open.length)}`, j.open.length ? h("div", { class: "ea-list" }, j.open.map((a) => h("div", { class: a.severity, onclick: () => app.go("fire", a.fire) }, h("span", { class: "t" }, a.title), h("span", { class: "v" }, REGION[a.region]?.title || ""), h("span", { class: "m" }, a.detail)))) : h("p", { class: "ea-empty" }, "None."));
}

async function data(app, page) {
  const j = await app.api("/v1/data");
  put(page, 
    card("Tables", "every row the app keeps is in ChloeDB", h("table", {}, h("tbody", {}, j.tables.map((t) => h("tr", {}, h("td", {}, h("code", {}, t.table)), h("td", { class: "num" }, fmt(t.rows)), h("td", { class: "ea-note" }, t.about)))))),
    card("Scans read", "", j.satellites.length ? h("table", {}, h("thead", {}, h("tr", {}, h("th", {}, "Region"), h("th", {}, "Satellite"), h("th", { class: "num" }, "Scans"), h("th", { class: "num" }, "Fire pixels"), h("th", {}, "From"), h("th", {}, "To"))), h("tbody", {}, j.satellites.map((s) => h("tr", {}, h("td", {}, REGION[s.region]?.title || s.region), h("td", {}, s.sat), h("td", { class: "num" }, fmt(s.scans)), h("td", { class: "num" }, fmt(s.pixels)), h("td", {}, when(s.first, s.region, { year: true })), h("td", {}, when(s.last, s.region, { year: true })))))) : h("p", { class: "ea-empty" }, "None yet.")),
    j.backtest?.length ? backtestCard(j.backtest) : null,
    card("How it's built", "", h("p", { class: "ea-sentence" }, "One ChloeDB server holds it all: the places with their 1,152-number SigLIP vectors (searched with ", h("code", {}, "ORDER BY similarity(look, :v)"), "), the words' model (", h("code", {}, "siglip-so400m-patch14-384"), ", which embeds a search or a picture on the server), every fire pixel and scan, the fires with their first hours as vectors (matched with ", h("code", {}, "ORDER BY distance(shape2, :v) ASC"), "), and the rules as SQL. The app is a client of it over HTTP; the SQL button shows every statement a page ran, with its time."),
      h("p", { class: "ea-note" }, `This answer: ${fmt(j.took_ms, 1)} ms. Places in the app's memory: ${fmt(j.places_in_memory)}.`)),
    card("Where the data comes from", "", h("table", {}, h("tbody", {}, j.sources.map((s) => h("tr", {}, h("td", {}, h("a", { href: s.url, target: "_blank", rel: "noopener" }, s.name)), h("td", {}, s.what), h("td", { class: "ea-note" }, s.licence)))))));
}

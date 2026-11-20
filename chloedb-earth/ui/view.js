// Earth on ChloeDB: California and Chile from space. The page, in ChloeDB's
// command center (#/app/earth, loaded through chloedb-server from this app)
// and on its own (the app's / when it has no token, through ui/index.html).
// It builds its own elements, so the same file serves both, and loads one
// library, MapLibre GL, from jsDelivr at a pinned version.
//
//   (home)                 what it is, both regions now, the replays
//   live/<region>          the fires GOES sees burning now, and its last hour
//   replay/<id>            a past day played back, scan by scan
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
const PX_HALF = 0.0115;

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

/// Fire pixels as small squares (GOES's pixels are about 2 km over
/// California and 3 km over central Chile), coloured by power.
function pixelFeatures(rows, half = 0.009) {
  return {
    type: "FeatureCollection",
    features: rows.map((p) => {
      const dl = half / Math.cos((p.lat * Math.PI) / 180);
      return { type: "Feature", properties: { mw: p.mw || 0, fire: p.fire || "", age: p.age ?? 0 }, geometry: { type: "Polygon", coordinates: square(p.lat, p.lon, half, dl) } };
    }),
  };
}
const PIXEL_PAINT = {
  "fill-color": ["interpolate", ["linear"], ["get", "mw"], 0, "#ffd54f", 20, "#ffa000", 80, "#f4511e", 250, "#d50000", 1000, "#7f0000"],
  "fill-opacity": ["interpolate", ["linear"], ["get", "age"], 0, 0.92, 1, 0.25],
  "fill-outline-color": "rgba(255,255,255,.55)",
};

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

  const head = h("div", { class: "ea-head" },
    h("h1", { html: `<svg viewBox="0 0 24 24">${ICON}</svg>Earth` }),
    h("span", { class: "sub" }, "California and Chile from space, on ChloeDB"),
    h("div", { class: "tools" }, h("button", { class: "btn sm", onclick: () => showSql(app) }, "SQL")));
  const tabs = h("nav", { class: "ea-tabs" });
  const body = h("div");
  root.append(head, tabs, body);

  const TABS = [["", "Home"], ["live", "Live"], ["explore", "Explore"], ["search", "Search"], ["twins", "Climate twins"], ["changed", "Changed places"], ["fires", "Past fires"], ["rules", "Rules"], ["data", "How it's built"]];
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
  return () => { token++; for (const s of app.stops.splice(0)) { try { s(); } catch { /* gone */ } } };
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
  const finder = h("form", { class: "ea-search", onsubmit: (e) => { e.preventDefault(); const f = found.querySelector("a"); if (f) f.click(); } },
    h("input", { type: "search", name: "q", placeholder: "Find a place: a town, a county, or 34.05, -118.2", oninput: async (e) => {
      const q = e.target.value.trim(), mine = ++t;
      if (q.length < 2) return swap(found);
      const r = await app.api(`/v1/find?q=${encodeURIComponent(q)}`, { keep: 60000 });
      if (mine === t) swap(found, r.found.map((x) => h("a", { href: app.href("place", x.id), class: "warn" }, `${x.label} · ${REGION[x.region]?.title || x.region}`)));
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
  // Satellite now: on by default, under the fire pixels and the names.
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
  let fitted = false;
  const draw = async (fresh) => {
    const j = await app.api(`/v1/live?region=${region}`, { fresh, keep: 15000 });
    const now = Date.parse(j.now) || 0;
    const pixels = j.pixels.map((p) => ({ ...p, age: now ? Math.min(1, (now - Date.parse(p.ts)) / 3600e3) : 0 }));
    setData(map, "px", pixelFeatures(pixels, PX_HALF), [{ id: "px", type: "fill", paint: PIXEL_PAINT }]);
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
      setData(map, "px", pixelFeatures(rows, PX_HALF), [{ id: "px", type: "fill", paint: PIXEL_PAINT }]);
    }
    tl.replaceChildren(h("div", { class: "ea-mapttl" }, h("b", {}, j.title), ` · ${R.sat} fire pixels`),
      h("div", { class: `ea-onair${s.playing ? "" : " off"}` }, h("span", { class: "light" }), h("b", {}, s.playing ? "REPLAY" : "PAUSED"), h("span", { class: "t" }, `real scans, ${speeds[s.speed]}× speed`)));
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
  const map = await makeMap(box, { center: [f.lon, f.lat], zoom: 10 });
  app.stops.push(() => map.remove());
  const t0 = Date.parse(f.start), t1 = Date.parse(f.last) || t0 + 1;
  const rows = j.pixels.map((p) => ({ ...p, age: Math.min(1, (Date.parse(p.ts) - t0) / Math.max(3600e3, t1 - t0)) * 0.7 }));
  setData(map, "px", pixelFeatures(rows, PX_HALF), [{ id: "px", type: "fill", paint: { ...PIXEL_PAINT, "fill-color": ["interpolate", ["linear"], ["get", "age"], 0, "#ffeb3b", 0.35, "#ff6d00", 0.7, "#8e0000"], "fill-opacity": 0.8 } }]);
  if (rows.length) {
    const b = new window.maplibregl.LngLatBounds();
    rows.forEach((p) => b.extend([p.lon, p.lat]));
    map.fitBounds(b, { padding: 60, maxZoom: 12, duration: 0 });
  }
  box.append(h("div", { class: "ea-maptl" }, h("div", { class: "ea-mapttl" }, h("b", {}, f.name), " · its pixels, yellow first"), f.state === "burning" && Date.now() - Date.parse(f.last) < 3 * 3600e3 ? onAir(true, "burning now") : null));
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
      j.place ? h("p", { style: { marginTop: "10px" } }, h("a", { href: app.href("place", j.place.id) }, "The place it started on →")) : null),
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
  const box = h("div", { class: "ea-map", style: { minHeight: "280px" } });
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
  put(page, h("div", { style: { marginTop: "14px" } }, placePanel(app, j)));
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
  const path = `/v1/chip/${encodeURIComponent(place)}/${year}`;
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

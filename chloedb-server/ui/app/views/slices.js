// Slices: the parts of a table that devices hold to search near their user
// (chloedb::slice), and how they are used. A slice is a manifest and the
// rows of each map tile (a geo slice) or of each vector cluster (a vector
// slice); a device fetches the tiles around its user, or the clusters
// nearest its user's taste, answers a search itself when it can prove its
// answer is the server's, and asks the server otherwise.
//
// GET /v1/admin/slices lists the slices, whether each table changed since
// its slice was built, and the counts; GET /v1/admin/slices/<table> adds
// each tile's (or cluster's) rows, bytes and downloads and the last hour by minute. The
// counts are the server's since it started: the manifests and tiles it
// served, and what devices reported of their searches (POST
// /v1/slices/<table>/usage). POST /v1/admin/slices builds or rebuilds one,
// DELETE /v1/admin/slices/<table> removes it.

import { h, icon, clear, fmt, api, toast, confirm, card, empty, failure, pill, every, can, needs, seg, showTip, hideTip, timeEl, store, modal, copy } from "../core.js";
import { timeChart, barList, donut } from "../charts.js";

export default {
  id: "slices", title: "Slices", icon: "layers", group: "Data", range: false,
  keywords: "slices slice device devices phone offline on-device edge tiles tile map geo near nearby location vector vectors cluster clusters taste similar recommendations manifest download cloud fallback proof proved fresh stale rebuild",
  render, onParams,
};

let reselect = null;
function onParams(r) { reselect?.(r.params.get("table")); }

/** Why a search was answered where it was, as devices report it. */
const WHY = [
  { key: "proved", name: "Proved on the device", color: "var(--good)", device: true, sub: "the k-th local score beat the most a row not held could score" },
  { key: "no_match", name: "No row matches", color: "var(--s3)", device: true, sub: "no row in the table has a word of the search, so the answer is empty everywhere" },
  { key: "short", name: "Too few matches held", color: "var(--s4)", device: false, sub: "the device held fewer than k rows that match the search" },
  { key: "bound", name: "Bound not reached", color: "var(--s2)", device: false, sub: "a row beyond the tiles or clusters held could have scored higher" },
  { key: "stale", name: "Slice stale", color: "var(--s8)", device: false, sub: "the table changed since the slice was built" },
];

const METRICS = [["rows", "Rows"], ["bytes", "Size"], ["downloads", "Downloads"]];
const isVec = (d) => d.kind === "vector";
/** What a slice's parts are called. */
const part = (d, n) => (isVec(d) ? (n === 1 ? "cluster" : "clusters") : (n === 1 ? "tile" : "tiles"));

function render(page, r) {
  const top = h("div", { class: "stack" });
  page.append(top);
  let alive = true, list = null, detail = null, pick = r.params.get("table") || store.get("slices.pick", null);
  let metric = store.get("slices.metric", "downloads");
  reselect = (t) => { if (t && t !== pick) { pick = t; detail = null; load(); } };

  async function load() {
    try {
      list = await api("/v1/admin/slices");
      const names = (list.slices || []).map((x) => x.table);
      if (!names.includes(pick)) pick = names[0] || null;
      detail = pick ? await api(`/v1/admin/slices/${encodeURIComponent(pick)}`) : null;
    } catch (e) { clear(top, failure(e, "Slices")); return; }
    if (alive) draw();
  }

  function draw() {
    if (!list.enabled) {
      clear(top, card({ title: "Slices", body: empty("layers", "This server builds no slices", "Start chloedb-server with --slices DIR. A slice is the part of a table a device holds to search near its user; the device answers a search itself when it can prove its answer is the server's, and asks the server otherwise.") }));
      return;
    }
    if (!list.slices.length) {
      clear(top, card({ title: "Slices", tools: [buildBtn("primary")], body: empty("layers", "No slice yet", "Build one from a table with a TEXT field and latitude and longitude columns (devices fetch the tiles around their user), or with a VECTOR field (devices fetch the clusters nearest their user's taste).", buildBtn("primary")) }));
      return;
    }
    const d = detail;
    clear(top, picker(), banner(d), kpis(d),
      h("div", { class: "grid g3" }, card({ cls: "span2", title: isVec(d) ? "Clusters" : "Tiles", sub: isVec(d) ? `${fmt.n(d.tiles)} k-means clusters of ${d.field}, busiest first` : `${fmt.n(d.tiles)} tiles of ${fmt.n(d.grid.tile_m)} m`, tools: [seg(METRICS, metric, (v) => { metric = v; store.set("slices.metric", v); draw(); })], body: tileMap(d) }), whyCard(d)),
      h("div", { class: "grid g2" }, hourCard(d), hotCard(d)),
      h("div", { class: "grid g2" }, freshCard(d), useCard(d)));
  }

  function picker() {
    if (list.slices.length < 2) return h("div", { class: "row between" }, h("div", { class: "small dim" }, "Slices of this server's tables, and how devices use them since it started."), buildBtn());
    return h("div", { class: "row between" }, seg(list.slices.map((x) => [x.table, x.table]), pick, (t) => { pick = t; store.set("slices.pick", t); history.replaceState(null, "", `#/slices?table=${encodeURIComponent(t)}`); detail = null; load(); }), buildBtn());
  }

  function buildBtn(kind = "") {
    const b = h("button", { class: "btn sm " + kind, onclick: () => buildForm(list, null, load) }, icon("plus"), "Build a slice");
    if (!can("admin")) { b.disabled = true; b.title = needs("admin"); }
    return b;
  }

  function banner(d) {
    const u = d.usage;
    const share = u.searches ? u.on_device / u.searches : null;
    const tone = !d.fresh ? "warn" : "ok";
    const what = !d.fresh
      ? `Table ${d.table} changed ${fmt.ago(d.changed_ms)}, after this slice was built: devices that check ask the server until it is rebuilt.`
      : u.searches ? `Devices answered ${fmt.pct(share)} of ${fmt.n(u.searches)} searches themselves, and asked the server for the rest.` : "Fresh. No device has reported a search yet.";
    return h("div", { class: "banner " + tone, style: { margin: 0 } }, icon(d.fresh ? "okc" : "warn", "lg"),
      h("div", { class: "grow" }, h("div", { class: "big" }, isVec(d) ? `${d.table}: ${fmt.n(d.rows)} rows in ${fmt.n(d.tiles)} clusters of ${d.field}` : `${d.table}: ${fmt.n(d.rows)} places in ${fmt.n(d.tiles)} tiles`), h("div", { class: "small dim" }, what)),
      pill(d.fresh ? "ok" : "warn", d.fresh ? "fresh" : "stale"));
  }

  function kpis(d) {
    const u = d.usage;
    const k = (label, val, unit, sub) => h("div", { class: "card kpi" }, h("div", { class: "lbl" }, label), h("div", { class: "val" }, val, unit ? h("small", {}, unit) : null), h("div", { class: "small dim" }, sub));
    return h("div", { class: "grid g4" },
      k("Answered on the device", u.searches ? fmt.pct(u.on_device / u.searches) : "–", "", u.searches ? `${fmt.n(u.on_device)} of ${fmt.n(u.searches)} searches · ${fmt.ms(u.device_ms)} each` : "no search reported yet"),
      k("Devices", fmt.n(u.devices), "", `${fmt.plural(u.reports, "report")}${u.last_ms ? ` · last ${fmt.ago(u.last_ms)}` : ""}`),
      k(isVec(d) ? "Clusters downloaded" : "Tiles downloaded", fmt.si(u.tile_downloads), "", `${fmt.bytes(u.tile_bytes + u.manifest_bytes + (u.codes_bytes || 0))} served · ${fmt.plural(u.manifests, "manifest")}${isVec(d) && d.codes_bytes ? ` · codes ${fmt.n(u.codes || 0)}×` : ""}`),
      k("Slice size", fmt.bytes(d.manifest_bytes + d.tile_bytes + (d.codes_bytes || 0)), "", `manifest ${fmt.bytes(d.manifest_bytes)} · ${isVec(d) ? `${fmt.n(d.dim)} dimensions${d.codes_bytes ? ` · codes ${fmt.bytes(d.codes_bytes)}` : " · no codes"}` : `${fmt.n(d.words)} words`} · largest ${part(d, 1)} ${fmt.bytes(d.largest_tile_bytes)}`));
  }

  /** The tiles on a map of the grid, shaded by the metric chosen; a
   * vector slice's clusters, which have no place on a map, as a grid of
   * cells in order of the metric (the busiest at the top left). */
  function tileMap(d) {
    let tiles = (d.tile_list || []).map(([x, y, rows, bytes, downloads]) => ({ x, y, rows, bytes, downloads }));
    if (!tiles.length) return empty("layers", `No ${part(d, 2)}`, "The table had no rows when the slice was built.");
    const val = (t) => t[metric];
    if (isVec(d)) {
      // Laid out as a grid about twice as wide as it is tall.
      tiles.sort((a, b) => val(b) - val(a) || a.x - b.x);
      const cols = Math.max(1, Math.ceil(Math.sqrt(tiles.length * 2)));
      tiles = tiles.map((t, i) => ({ ...t, gx: i % cols, gy: -Math.floor(i / cols) }));
    } else {
      tiles = tiles.map((t) => ({ ...t, gx: t.x, gy: t.y }));
    }
    const xs = tiles.map((t) => t.gx), ys = tiles.map((t) => t.gy);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const max = Math.max(1, ...tiles.map(val));
    const one = part(d, 1), many = part(d, 2);
    const wrap = h("div", { class: "sl-map" });
    const cv = h("canvas", { "aria-label": isVec(d) ? "the slice's clusters, busiest first" : "the slice's tiles on a map" });
    const legend = h("div", { class: "row small dim sl-legend" },
      h("span", {}, metric === "downloads" ? `downloads per ${one}` : metric === "rows" ? `rows per ${one}` : `bytes per ${one}`),
      h("span", { class: "sl-ramp" }), h("span", {}, "0 – " + (metric === "bytes" ? fmt.bytes(max) : fmt.si(max))),
      metric === "downloads" ? h("span", {}, h("span", { class: "sl-none" }), " never downloaded") : null,
      h("span", { class: "grow" }), h("span", {}, `${fmt.n(tiles.filter((t) => t.downloads > 0).length)} of ${fmt.n(tiles.length)} ${many} downloaded at least once`));
    wrap.append(cv, legend);
    let geo = null;
    const draw = () => {
      const w = Math.max(240, wrap.clientWidth || 600);
      const nx = x1 - x0 + 1, ny = y1 - y0 + 1;
      const cell = Math.max(2, Math.min(28, Math.floor(w / nx), Math.floor(460 / ny)));
      const W = nx * cell, H = ny * cell;
      const dpr = window.devicePixelRatio || 1;
      cv.width = W * dpr; cv.height = H * dpr; cv.style.width = W + "px"; cv.style.height = H + "px";
      const g = cv.getContext("2d");
      g.scale(dpr, dpr);
      const css = getComputedStyle(cv);
      const accent = css.getPropertyValue("--s1").trim() || "#2a78d6", line = css.getPropertyValue("--line-2").trim() || "#ccc", bg = css.getPropertyValue("--surface-2").trim() || "#eee";
      g.fillStyle = bg; g.fillRect(0, 0, W, H);
      for (const t of tiles) {
        const px = (t.gx - x0) * cell, py = (y1 - t.gy) * cell;
        const v = val(t);
        if (metric === "downloads" && v === 0) {
          g.strokeStyle = line; g.lineWidth = 1; g.strokeRect(px + 0.5, py + 0.5, cell - 1, cell - 1);
          continue;
        }
        // A log scale: a few busy tiles would wash the rest out.
        const a = 0.12 + 0.88 * Math.log1p(v) / Math.log1p(max);
        g.globalAlpha = a; g.fillStyle = accent; g.fillRect(px, py, cell - (cell > 4 ? 1 : 0), cell - (cell > 4 ? 1 : 0));
        g.globalAlpha = 1;
      }
      geo = { cell, by: new Map(tiles.map((t) => [t.gx + "," + t.gy, t])) };
    };
    cv.addEventListener("pointermove", (e) => {
      if (!geo) return;
      const r = cv.getBoundingClientRect();
      const tx = x0 + Math.floor((e.clientX - r.left) / geo.cell), ty = y1 - Math.floor((e.clientY - r.top) / geo.cell);
      const t = geo.by.get(tx + "," + ty);
      if (!t) { hideTip(); return; }
      const km = (v) => (v * d.grid.tile_m / 1000).toFixed(1);
      showTip(e.clientX, e.clientY, [h("div", { class: "t" }, isVec(d) ? `cluster ${t.x} · ${fmt.pct(t.rows / (d.rows || 1))} of the rows` : `tile ${t.x}, ${t.y} · ${km(t.x + 0.5)} km east, ${km(t.y + 0.5)} km north of the origin`),
        h("div", { class: "r" }, h("b", {}, fmt.n(t.rows)), h("span", { class: "n" }, "rows")),
        h("div", { class: "r" }, h("b", {}, fmt.bytes(t.bytes)), h("span", { class: "n" }, "gzipped")),
        h("div", { class: "r" }, h("b", {}, fmt.n(t.downloads)), h("span", { class: "n" }, "downloads"))]);
    });
    cv.addEventListener("pointerleave", hideTip);
    const ro = new ResizeObserver(() => draw());
    ro.observe(wrap);
    requestAnimationFrame(draw);
    return wrap;
  }

  function whyCard(d) {
    const u = d.usage;
    const items = WHY.map((w) => ({ name: w.name, value: u.why?.[w.key] || 0, color: w.color, sub: w.sub }));
    const body = u.searches
      ? h("div", { class: "stack" },
          h("div", { class: "row nw", style: { gap: "16px" } }, donut([{ name: "on the device", value: u.on_device, color: "var(--good)" }, { name: "asked the server", value: u.asked, color: "var(--s4)" }], { size: 120, thick: 16, center: fmt.pct(u.on_device / u.searches, 0), sub: "on device" }),
            h("div", { class: "small dim" }, `${fmt.n(u.asked)} searches went to the server`, u.cloud_ms != null ? `, ${fmt.ms(u.cloud_ms)} each as devices timed them` : "", ".")),
          barList(items, { fmtv: fmt.n }),
          h("div", { class: "small dim" }, WHY.filter((w) => !w.device).map((w) => h("div", {}, h("b", {}, w.name), ": ", w.sub))))
      : empty("info", "No searches reported", "Devices report their searches to POST /v1/slices/<table>/usage; the Python and JavaScript helpers do it every 20 searches.");
    return card({ title: "Where searches were answered", body });
  }

  function hourCard(d) {
    if (!(d.minutes || []).length) return card({ title: "The last hour", body: empty("clock", "Nothing yet", `Searches devices report and ${part(d, 2)} they download show here by minute.`) });
    // Every minute of the hour, each drawn as a step as wide as the
    // minute: a minute with searches reads as a bar, not a spike.
    const now = Date.now(), MIN = 60e3, byMin = new Map(d.minutes.map((m) => [Math.floor(m.at_ms / MIN), m]));
    const pts = [];
    for (let i = Math.floor((now - 3600e3) / MIN); i <= Math.floor(now / MIN); i++) {
      const m = byMin.get(i) || { searches: 0, on_device: 0, tiles: 0, bytes: 0 };
      const p = { on: m.on_device, asked: Math.max(0, m.searches - m.on_device), tiles: m.tiles, bytes: m.bytes };
      pts.push({ ...p, t: i * MIN }, { ...p, t: (i + 1) * MIN - 1 });
    }
    const searches = timeChart({ height: 150, label: "searches by minute", stack: true, domain: () => [now - 3600e3, now], series: [{ name: "on the device", get: (p) => p.on, color: "var(--good)", area: true }, { name: "asked the server", get: (p) => p.asked, color: "var(--s4)", area: true }] });
    const tiles = timeChart({ height: 110, label: `${part(d, 2)} downloaded by minute`, domain: () => [now - 3600e3, now], legend: false, series: [{ name: `${part(d, 2)} downloaded`, get: (p) => p.tiles, color: "var(--s1)", area: true }] });
    requestAnimationFrame(() => { searches.update(pts); tiles.update(pts); });
    return card({ title: "The last hour", sub: "by minute", body: h("div", { class: "stack" }, h("div", { class: "small dim" }, "Searches reported"), searches, h("div", { class: "small dim" }, isVec(d) ? "Clusters downloaded" : "Tiles downloaded"), tiles) });
  }

  function hotCard(d) {
    const tiles = (d.tile_list || []).map(([x, y, rows, bytes, downloads]) => ({ x, y, rows, bytes, downloads })).sort((a, b) => b.downloads - a.downloads || b.rows - a.rows).slice(0, 8);
    const rowsTotal = d.rows || 1;
    return card({ title: isVec(d) ? "Busiest clusters" : "Busiest tiles", sub: "by downloads", flush: true, body: h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" },
      h("thead", {}, h("tr", {}, h("th", {}, part(d, 1)), h("th", { class: "num" }, "downloads"), h("th", { class: "num" }, "rows"), h("th", { class: "num" }, "share of rows"), h("th", { class: "num" }, "size"))),
      h("tbody", {}, tiles.map((t) => h("tr", {}, h("td", { class: "mono" }, isVec(d) ? `${t.x}` : `${t.x}, ${t.y}`), h("td", { class: "num" }, fmt.n(t.downloads)), h("td", { class: "num" }, fmt.n(t.rows)), h("td", { class: "num" }, fmt.pct(t.rows / rowsTotal)), h("td", { class: "num" }, fmt.bytes(t.bytes))))))) });
  }

  function freshCard(d) {
    const rebuild = h("button", { class: "btn sm", onclick: async () => {
      rebuild.disabled = true;
      try { const r = await api("/v1/admin/slices", { method: "POST", body: { table: d.table } }); toast(`Rebuilt ${d.table}: ${fmt.n(r.rows)} rows in ${fmt.n(r.tiles)} ${part(d, 2)}, ${fmt.ms(r.took_ms)}`, "ok"); load(); }
      catch (e) { toast(e.message, "bad"); rebuild.disabled = false; }
    } }, icon("refresh"), "Rebuild");
    const del = h("button", { class: "btn sm danger", onclick: async () => {
      if (!await confirm({ title: `Delete the slice of ${d.table}?`, body: "Devices then find no slice and ask the server for every search. The table is not touched.", ok: "Delete", danger: true })) return;
      try { await api(`/v1/admin/slices/${encodeURIComponent(d.table)}`, { method: "DELETE" }); toast(`Deleted the slice of ${d.table}`, "ok"); pick = null; load(); }
      catch (e) { toast(e.message, "bad"); }
    } }, icon("trash"), "Delete");
    for (const b of [rebuild, del]) if (!can("admin")) { b.disabled = true; b.title = needs("admin"); }
    return card({ title: "Version", tools: [rebuild, del], body: h("div", { class: "stack" },
      h("dl", { class: "kv" },
        h("dt", {}, "built"), h("dd", {}, timeEl(d.built_ms), h("span", { class: "dim" }, ` · version ${fmt.n(d.version)}`)),
        h("dt", {}, "table changed"), h("dd", {}, d.fresh ? h("span", { class: "dim" }, "not since the build") : timeEl(d.changed_ms)),
        ...(isVec(d)
          ? [h("dt", {}, "searched field"), h("dd", { class: "mono" }, `${d.field} · VECTOR(${d.dim})`),
             h("dt", {}, "clusters"), h("dd", {}, `${fmt.n(d.tiles)} by k-means · a device bounds each one it lacks by its centre, radius and angle${d.codes_bytes ? `, and by its rows' codes (${fmt.bytes(d.codes_bytes)}, one byte a dimension)` : ""}`)]
          : [h("dt", {}, "searched field"), h("dd", { class: "mono" }, `${d.text} · ${d.lat}, ${d.lon}`),
             h("dt", {}, "grid"), h("dd", {}, `origin ${d.grid.lat0.toFixed(4)}, ${d.grid.lon0.toFixed(4)} · tiles ${fmt.n(d.grid.tile_m)} m · BM25 k1 ${+d.k1.toFixed(3)}, b ${+d.b.toFixed(3)}`)])),
      h("div", { class: "small dim" }, "A device asks GET /v1/slices/", d.table, "/status before it trusts its slice: one of another version is fetched again, and while the table has changed since the build it asks the server. chloedb-server --slices-rebuild-s 300 rebuilds a stale slice every five minutes.")) });
  }

  function useCard(d) {
    const url = location.origin;
    const code = isVec(d) ? {
      python: `import strata\n\nnear = strata.nearby("${url}", "${d.table}", "./slice")\nnear.focus(taste, rows=20_000)   # the clusters nearest the user's taste\nr = near.similar(taste, k=10)    # or any vector near it\nprint(r.where, r.why)            # "device" or "server", and why\nprint(r.keys)`,
      javascript: `import { openNearby } from "chloedb";\n\nconst near = await openNearby("${url}", "${d.table}", "./slice");\nawait near.focus(taste, { rows: 20000 });  // the clusters nearest the user's taste\nconst r = await near.similar(taste, { k: 10 });\nconsole.log(r.where, r.why);  // "device" or "server", and why\nconsole.log(r.keys);`,
      rust: `let mut dev = chloedb::slice::Device::create(dir, manifest)?;\nfor (tile, _) in dev.plan_vector(&taste, 20_000)?.fetch { dev.add_tile(tile, &get(tile.path())?)?; }\nlet s = VectorSearch::new(taste.clone(), 10)?;\nlet out = dev.search_or_ask(&s, |sql, params| ask_server(sql, params))?;\n// out.answer.on_device, out.answer.why`,
    } : {
      python: `import strata\n\nnear = strata.nearby("${url}", "${d.table}", "./slice")\nnear.move_to(${d.grid.lat0.toFixed(4)}, ${d.grid.lon0.toFixed(4)})   # fetches the tiles around the user\nr = near.search("pizza", k=10)\nprint(r.where, r.why)        # "device" or "server", and why\nfor row in r: print(row["${d.text}"])`,
      javascript: `import { openNearby } from "chloedb";\n\nconst near = await openNearby("${url}", "${d.table}", "./slice");\nawait near.moveTo(${d.grid.lat0.toFixed(4)}, ${d.grid.lon0.toFixed(4)});  // fetches the tiles around the user\nconst r = await near.search("pizza", { k: 10 });\nconsole.log(r.where, r.why);  // "device" or "server", and why\nfor (const row of r) console.log(row.${d.text});`,
      rust: `let mut dev = chloedb::slice::Device::create(dir, manifest)?;\nfor (tile, _) in dev.plan(lat, lon, 2).fetch { dev.add_tile(tile, &get(tile.path())?)?; }\nlet s = Search::new("pizza", lat, lon, 10, 1000.0)?;\nlet out = dev.search_or_ask(&s, |sql, params| ask_server(sql, params))?;\n// out.answer.on_device, out.answer.why`,
    };
    let lang = store.get("slices.lang", "python");
    const pre = h("pre", { class: "code sl-code" });
    const show = () => { pre.textContent = code[lang]; };
    show();
    return card({ title: "Use it from an app", tools: [seg([["python", "Python"], ["javascript", "JavaScript"], ["rust", "Rust"]], lang, (v) => { lang = v; store.set("slices.lang", v); show(); }), h("button", { class: "btn sm ghost", onclick: () => copy(code[lang]) }, icon("copy"))],
      body: h("div", { class: "stack" }, pre, h("div", { class: "small dim" }, isVec(d)
        ? "The helper checks the slice's status, fetches the codes of every row and the clusters whose rows could score highest for the user's taste, answers on the device when it can prove the answer (fetching the few clusters the codes name, when that is all it lacks), asks the server otherwise (an exact search), and reports its counts here."
        : "The helper checks the slice's status, fetches the tiles around the user as it moves, answers on the device when it can prove the answer, asks the server otherwise, and reports its counts here.")) });
  }

  load();
  const stop = every(10_000, () => { if (list) load(); }, false);
  return () => { alive = false; stop(); reselect = null; };
}

/** The form that builds a slice: of places (a table, its TEXT field, its
 * latitude and longitude, and the grid) or of vectors (a table, its VECTOR
 * field, and the number of clusters). */
function buildForm(list, _d, done) {
  const all = list.tables || [];
  const geoTables = all.filter((t) => t.fields.some((f) => f.type === "TEXT") && t.fields.filter((f) => f.type === "FLOAT").length >= 2);
  const vecTables = all.filter((t) => t.fields.some((f) => f.type === "VECTOR"));
  if (!geoTables.length && !vecTables.length) { toast("No table has a TEXT field and two FLOAT columns for latitude and longitude, or a VECTOR field", "warn"); return; }
  let kind = geoTables.length ? "geo" : "vector";
  const table = h("select", { class: "select" });
  const text = h("select", { class: "select" }), lat = h("select", { class: "select" }), lon = h("select", { class: "select" });
  const lat0 = h("input", { class: "input", type: "number", step: "any" }), lon0 = h("input", { class: "input", type: "number", step: "any" });
  const vfield = h("select", { class: "select" });
  const clusters = h("input", { class: "input", type: "number", min: "0", step: "1", placeholder: "automatic" });
  const codes = h("input", { type: "checkbox", checked: true });
  let tileM = 1000;
  const set = (e, names, want) => { clear(e, names.map((n) => h("option", { value: n }, n))); e.value = (want && names.find((n) => want.test(n))) || names[0]; };
  const tables = () => (kind === "geo" ? geoTables : vecTables);
  const fill = () => {
    const t = tables().find((x) => x.table === table.value);
    const of = (ty) => t.fields.filter((f) => f.type === ty).map((f) => f.name);
    if (kind === "geo") {
      set(text, of("TEXT"), /body|text|name|title/i);
      set(lat, of("FLOAT"), /^lat/i);
      set(lon, of("FLOAT"), /^(lon|lng)/i);
      middle();
    } else {
      set(vfield, of("VECTOR"), /emb|vec/i);
    }
  };
  const middle = async () => {
    try {
      const r = await api("/v1/query", { method: "POST", body: { sql: `SELECT AVG(${lat.value}), AVG(${lon.value}) FROM ${table.value}` } });
      const [a, o] = r.rows?.[0] || [];
      if (Number.isFinite(a) && Number.isFinite(o)) { lat0.value = a.toFixed(4); lon0.value = o.toFixed(4); }
    } catch { /* left for the person to fill */ }
  };
  table.onchange = fill; lat.onchange = middle; lon.onchange = middle;
  const f = (label, el) => h("label", { class: "f" }, label, el);
  const fields = h("div", { class: "stack" });
  const show = () => {
    set(table, tables().map((t) => t.table));
    fill();
    clear(fields, kind === "geo"
      ? [h("div", { class: "small dim" }, "Writes the table's manifest and the rows of each map tile under --slices, from one snapshot. A device fetches the tiles around its user. A slice of the same table replaces the old one once it is whole."),
         h("div", { class: "grid g2" }, f("Table", table), f("Searched TEXT field", text), f("Latitude", lat), f("Longitude", lon), f("Grid origin latitude", lat0), f("Grid origin longitude", lon0)),
         f("Tile side", seg([[500, "500 m"], [1000, "1 km"], [2000, "2 km"]], tileM, (v) => { tileM = v; })),
         h("div", { class: "small dim" }, "The origin defaults to the middle of the table's places. Distances are measured on a flat projection around it, so keep it near the middle of the city.")]
      : [h("div", { class: "small dim" }, "Clusters the table's rows by k-means and writes the manifest (each cluster's centre and extent) and the rows of each cluster under --slices, from one snapshot. A device fetches the clusters nearest its user's taste and answers a similarity search itself when no cluster it lacks could hold a better row."),
         h("div", { class: "grid g2" }, f("Table", table), f("VECTOR field", vfield), f("Clusters", clusters)),
         h("label", { class: "row small" }, codes, "Every row's codes, one byte a dimension: a device that fetches them proves far more answers itself, and names the few clusters that would prove the rest"),
         h("div", { class: "small dim" }, "Clusters left empty: about the square root of the rows (at most 4,096). More clusters make each download smaller and the bounds tighter, and the manifest larger.")]);
  };
  const kinds = [geoTables.length ? ["geo", "Places near a position"] : null, vecTables.length ? ["vector", "Vectors by similarity"] : null].filter(Boolean);
  const kindSeg = kinds.length > 1 ? f("Kind", seg(kinds, kind, (v) => { kind = v; show(); })) : null;
  show();
  const go = h("button", { class: "btn primary", onclick: async () => {
    go.disabled = true;
    const body = kind === "geo"
      ? { table: table.value, text: text.value, lat: lat.value, lon: lon.value, lat0: Number(lat0.value), lon0: Number(lon0.value), tile_m: tileM }
      : { table: table.value, vector: vfield.value, codes: codes.checked, ...(clusters.value ? { clusters: Number(clusters.value) } : {}) };
    try {
      const r = await api("/v1/admin/slices", { method: "POST", body });
      toast(`Built ${r.table}: ${fmt.n(r.rows)} rows in ${fmt.n(r.tiles)} ${kind === "geo" ? "tiles" : "clusters"}, ${fmt.bytes(r.manifest_bytes + r.tile_bytes)}, ${fmt.ms(r.took_ms)}`, "ok");
      m.close(); done();
    } catch (e) { toast(e.message, "bad"); go.disabled = false; }
  } }, icon("layers"), "Build");
  const m = modal({
    title: "Build a slice",
    body: h("div", { class: "stack" }, kindSeg, fields),
    foot: [go],
  });
}

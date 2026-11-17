// The disk under a copy, in a drawer (Home's problems open it): how fast it
// read over the last hour (its probe's readings, GET /v1/admin/disk), how
// full it is against the line where Kubernetes evicts the node's pods, and
// what fills it: the database's own files apart from everything else
// beside them. A person may delete entries of the second kind, after
// reading what they are and typing the copy's name; the server deletes
// only those, as they were listed, and never its own.

import { h, icon, clear, fmt, api, drawer, confirm, toast, copyName, when, can, needs } from "./core.js";

const CLEAN = "POST /v1/admin/disk/clean";
const PROBE = "POST /v1/admin/disk/probe";

/** Bars of the probe's readings, the latest right: random reads in ms. */
function readings(kept) {
  const xs = (kept || []).slice(-60);
  if (!xs.length) return h("div", { class: "small muted" }, "No reading yet: the probe reads within a minute of the start, then every minute.");
  const max = Math.max(...xs.map((r) => r.read_ms), 0.001);
  const w = 6, gap = 2, hgt = 48;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${xs.length * (w + gap)} ${hgt}`);
  svg.setAttribute("preserveAspectRatio", "none");
  svg.setAttribute("class", "disk-bars");
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", `random reads over the last ${xs.length} probes, the slowest ${fmt.ms(max)}`);
  xs.forEach((r, i) => {
    const bh = Math.max(1, (r.read_ms / max) * (hgt - 2));
    const b = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    b.setAttribute("x", i * (w + gap)); b.setAttribute("y", hgt - bh); b.setAttribute("width", w); b.setAttribute("height", bh);
    b.setAttribute("fill", r.scrubbing ? "var(--muted)" : "var(--s1)");
    const t = document.createElementNS("http://www.w3.org/2000/svg", "title");
    t.textContent = `${fmt.time(r.at_ms)}: ${fmt.ms(r.read_ms)} a read (slowest ${fmt.ms(r.read_max_ms)})${r.mb_s != null ? `, ${fmt.n(r.mb_s)} MB/s in a row` : ""}${r.busy != null ? `, busy ${fmt.pct(r.busy, 0)}` : ""}${r.scrubbing ? ", during a scrub" : ""}`;
    b.append(t); svg.append(b);
  });
  return h("div", { class: "stack", style: { gap: "4px" } }, svg, h("div", { class: "small dim" }, `the last ${fmt.plural(xs.length, "probe")}, a bar each, the latest right · the slowest ${fmt.ms(max)}`));
}

/** The file system as a bar, with the eviction line marked when the kubelet shares it. */
function fullness(fs, onNode) {
  if (!fs) return h("div", { class: "small muted" }, "The file system's size is not known here.");
  const used = fs.used_bytes / Math.max(1, fs.total_bytes);
  const line = fs.evict_free_ratio != null ? 1 - fs.evict_free_ratio : null;
  const bad = fs.evict_headroom_bytes != null && fs.evict_headroom_bytes <= 0;
  const near = fs.evict_headroom_bytes != null && fs.evict_headroom_bytes < 0.05 * fs.total_bytes;
  const bar = h("div", { class: "disk-fill", role: "img", "aria-label": `${fmt.pct(used, 0)} used${line != null ? `, the eviction line at ${fmt.pct(line, 0)}` : ""}` },
    h("div", { class: "disk-used " + (bad ? "crit" : near ? "warn" : ""), style: { width: `${Math.min(100, used * 100)}%` } }),
    line != null ? h("div", { class: "disk-line", style: { left: `${line * 100}%` }, title: `Kubernetes evicts the node's pods past ${fmt.pct(line, 0)} used` }) : null);
  const lines = [`${fmt.bytes(fs.used_bytes)} of ${fmt.bytes(fs.total_bytes)} used (${fmt.pct(used, 0)}), ${fmt.bytes(fs.available_bytes)} free`];
  if (fs.evict_headroom_bytes != null) lines.push(fs.evict_headroom_bytes <= 0
    ? `past the line where Kubernetes evicts every pod of the node (${fmt.pct(fs.evict_free_ratio, 0)} free): free space now`
    : `${fmt.bytes(fs.evict_headroom_bytes)} above the line where Kubernetes evicts every pod of the node (${fmt.pct(fs.evict_free_ratio, 0)} free)`);
  else lines.push(onNode === false ? "on a disk of its own: the kubelet does not evict for it" : "not under Kubernetes: no eviction line");
  return h("div", { class: "stack", style: { gap: "6px" } }, bar, lines.map((t, i) => h("div", { class: "small " + (i ? (bad ? "wl-bad" : near ? "wl-warn" : "dim") : "") }, t)));
}

/** Opens the drawer of the disk under `copy` (a URL; none: this server). */
export function diskDrawer(copy, { onChange } = {}) {
  const name = copy ? copyName(copy) : "this server";
  const q = copy ? `?copy=${encodeURIComponent(copy)}` : "";
  const d = drawer({ title: `The disk under ${name}`, sub: "how fast it reads, how full it is, and what fills it", wide: true, body: h("div", { class: "muted small" }, "Reading the disk…") });
  const picked = new Map();
  let last = null;
  async function load() {
    try { last = await api("/v1/admin/disk" + q); }
    catch (e) { clear(d.body, h("div", { class: "wl-bad" }, `Could not read the disk: ${e.message}`)); return; }
    draw(last);
  }
  async function probe(btn) {
    btn.disabled = true;
    try { const j = await api("/v1/admin/disk/probe", { body: copy ? { copy } : {} }); const r = j.reading || {}; toast(`${name}: ${fmt.ms(r.read_ms)} a read${r.mb_s != null ? `, ${fmt.n(r.mb_s)} MB/s in a row` : ""}`, "ok"); onChange?.(); await load(); }
    catch (e) { toast(`Not measured: ${e.message}`, "bad", 8000); btn.disabled = false; }
  }
  async function clean() {
    const list = [...picked.values()];
    const total = list.reduce((a, e) => a + e.bytes, 0);
    const ok = await confirm({
      title: `Delete ${fmt.plural(list.length, "entry", "entries")} from ${name}'s disk`, danger: true, ok: `Delete ${fmt.bytes(total)}`, typed: name, reason: "Why: kept in the audit log with the deletion",
      body: h("div", { class: "stack", style: { gap: "6px" } },
        h("div", {}, `These are not the database's files: ${name} did not write them and holds none of them open. Deleted, they cannot be brought back.`),
        h("ul", { class: "small mono", style: { margin: 0, paddingLeft: "18px", maxHeight: "180px", overflow: "auto" } }, list.map((e) => h("li", {}, `${rel(e.path)} · ${fmt.bytes(e.bytes)}${e.files > 1 ? ` · ${fmt.n(e.files)} files` : ""} · changed ${when(e.modified_ms)}`))),
        h("div", { class: "small dim" }, "The server deletes only these, as listed, and refuses any that changed since.")),
    });
    if (!ok) return;
    try {
      const j = await api("/v1/admin/disk/clean", { body: { ...(copy ? { copy } : {}), paths: list.map((e) => ({ path: e.path, modified_ms: e.modified_ms })) }, reason: ok.reason });
      const refused = j.refused || [];
      toast(`Deleted ${fmt.plural((j.deleted || []).length, "entry", "entries")} (${fmt.bytes(j.freed_bytes || 0)} freed)${refused.length ? `; ${refused.length} refused: ${refused[0].why}` : ""}`, refused.length ? "bad" : "ok", refused.length ? 10000 : 5000);
      picked.clear(); onChange?.(); await load();
    } catch (e) { toast(`Not deleted: ${e.message}`, "bad", 10000); }
  }
  const rel = (p) => last?.root && p.startsWith(last.root) ? p.slice(last.root.length).replace(/^\//, "") || p : p;
  function draw(j) {
    const kept = j.probe?.kept || [], r = kept[kept.length - 1];
    const mayProbe = can("admin", PROBE), mayClean = can("admin", CLEAN);
    const probeBtn = h("button", { class: "btn sm", disabled: !mayProbe, title: mayProbe ? "Reads 4 MiB of the database around the page cache: free" : needs("admin", PROBE) }, icon("refresh"), "Measure again");
    probeBtn.onclick = () => probe(probeBtn);
    const speed = h("section", { class: "stack", style: { gap: "8px" } },
      h("div", { class: "row", style: { justifyContent: "space-between", alignItems: "baseline" } }, h("h3", { style: { margin: 0 } }, "How fast it reads"), probeBtn),
      r ? h("div", {}, h("b", {}, `${fmt.ms(r.read_ms)} a 4 KiB read`), ` (the slowest of ${r.reads}: ${fmt.ms(r.read_max_ms)})`, r.mb_s != null ? `, ${fmt.n(r.mb_s)} MB/s in a row` : "", r.busy != null ? ` · the device busy ${fmt.pct(r.busy, 0)} of the time since the probe before` : "", ` · ${when(r.at_ms)}`) : null,
      j.probe?.error ? h("div", { class: "small wl-bad" }, `The last probe failed ${when(j.probe.error.at_ms)}: ${j.probe.error.message}`) : null,
      readings(kept),
      h("div", { class: "small dim" }, "Read by the server itself from its database's file, around the page cache, as its queries read what the cache does not hold."));
    const room = h("section", { class: "stack", style: { gap: "8px" } }, h("h3", { style: { margin: 0 } }, "How full it is"), fullness(j.filesystem, j.kubernetes ? j.on_node : undefined));
    const entries = j.entries || [];
    const pick = (e, on) => { if (on) picked.set(e.path, e); else picked.delete(e.path); sync(); };
    const delBtn = h("button", { class: "btn danger sm", disabled: true, title: mayClean ? "" : needs("admin", CLEAN) }, icon("trash"), "Delete selected…");
    delBtn.onclick = clean;
    const sel = h("span", { class: "small dim" });
    function sync() {
      const n = picked.size, b = [...picked.values()].reduce((a, e) => a + e.bytes, 0);
      delBtn.disabled = !mayClean || !n;
      delBtn.textContent = ""; delBtn.append(icon("trash"), n ? `Delete ${fmt.plural(n, "entry", "entries")} (${fmt.bytes(b)})…` : "Delete selected…");
      sel.textContent = n ? "" : "pick what is not needed: never the database's own";
    }
    const rows = entries.map((e) => {
      const own = e.kind === "own";
      const box = h("input", { type: "checkbox", disabled: own || e.partial || !mayClean, "aria-label": `Pick ${rel(e.path)}`, checked: picked.has(e.path) });
      box.onchange = () => pick(e, box.checked);
      return h("tr", { class: own ? "dim" : "" },
        h("td", {}, own || e.partial ? null : box),
        h("td", { class: "mono small", title: e.path }, rel(e.path) + (e.dir ? "/" : "")),
        h("td", { class: "num" }, fmt.bytes(e.bytes)),
        h("td", { class: "num small" }, e.dir ? fmt.n(e.files) : ""),
        h("td", { class: "small" }, when(e.modified_ms)),
        h("td", { class: "small" }, own ? h("span", { class: "pill" }, "the database's") : e.partial ? h("span", { class: "pill warn", title: "Too large to list whole: delete it on the machine" }, "partly listed") : h("span", { class: "dim" }, e.why || "")));
    });
    const fills = h("section", { class: "stack", style: { gap: "8px" } },
      h("h3", { style: { margin: 0 } }, "What fills it"),
      h("div", { class: "small" }, `The database: ${fmt.bytes(j.own_bytes)}. Beside it, not the server's: `, h("b", {}, fmt.bytes(j.other_bytes)), `, in ${j.root || "its folder"}${j.complete === false ? ` (the listing stopped after ${fmt.n(j.walked)} entries: there may be more)` : ""}.`),
      entries.length ? h("div", { class: "tablewrap" }, h("table", { class: "tbl compact" },
        h("thead", {}, h("tr", {}, h("th", {}, ""), h("th", {}, "entry"), h("th", { class: "num" }, "size"), h("th", { class: "num" }, "files"), h("th", {}, "changed"), h("th", {}, ""))),
        h("tbody", {}, rows))) : h("div", { class: "small muted" }, "Nothing beside the database."),
      j.more ? h("div", { class: "small dim" }, `and ${fmt.plural(j.more, "smaller entry", "smaller entries")}`) : null,
      h("div", { class: "row", style: { gap: "8px", alignItems: "center" } }, delBtn, sel),
      h("div", { class: "small dim" }, "Files elsewhere on the node (images, other pods' files, logs) are not under the database's folder: clean them on the node."));
    clear(d.body, h("div", { class: "stack", style: { gap: "18px" } }, speed, room, fills));
    sync();
  }
  load();
  return d;
}

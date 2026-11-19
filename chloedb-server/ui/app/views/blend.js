// Blending (/v1/blends): one list made from the rows of several searches.
// Each blend is a stored function (CREATE BLEND), versioned; this page
// edits a draft of one and runs it as it changes (POST /v1/blends/preview,
// nothing stored), shows each row's slot and why, the rows passed over, the
// quotas' counts and what each rule costs, and saves the draft as the next
// version. Pins are dragged onto slots, quotas are two handles on a track.
// "Value on the log" sets the draft against the serving version over the
// exploring pages the blend logged (POST /v1/blends/:name/evaluate: cascade
// doubly robust over each slot's chance, SNIPS beside it), and tunes its
// weights and shares toward what people clicked (/tune).

import { h, s as svg, icon, clear, api, toast, modal, confirm, seg, fmt, empty, card, failure, gate, can, copy, sqlBlock, debounce, clamp, menu, drawer, store } from "../core.js";
import { SERIES } from "../charts.js";
import { tables } from "../catalog.js";
import { isLog, guessEvents } from "./recsys.js";

export default { id: "blend", title: "Blending", icon: "sliders", group: "Labs", range: false, keywords: "blend blending blender utility layer after ranking mix sources federated search results page slate heads relevance engagement recency calibrate tmm quota quotas share top k pin slot spacing diversity diversify mmr dedupe near duplicates cap per author receipts rule cost versions train tune off-policy evaluation counterfactual doubly robust snips propensity explore clicks", render, onParams };

let reshow = null;
function onParams(r) { reshow?.(r); }

// Drafts not stored yet, by blend name (a new blend's first draft).
const DRAFTS = {};
const clone = (x) => JSON.parse(JSON.stringify(x));
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const colorOf = (def, src) => SERIES[Math.max(0, def.sources.findIndex((s) => s.name === src)) % SERIES.length];
const HEAD_SHADES = [1, 0.72, 0.5, 0.34, 0.24, 0.16];
const CALS = [["tmm", "tmm", "Each source's best row is 1 (from 0, or LO)"], ["minmax", "minmax", "From each source's least row (0) to its best (1)"], ["rank", "rank", "Only the order inside a source counts: (60 + 1) / (60 + rank)"], ["raw", "raw", "The value as the source computed it"]];
const TITLE_COLS = ["title", "name", "text", "body", "label", "summary"];
const n2 = (x) => (x == null ? "–" : Number(x).toFixed(2));
const plain = (n) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(n);

function srcTag(def, name, extra) {
  return h("span", { class: "bl-tag" }, h("i", { style: { background: colorOf(def, name) } }), name, extra || null);
}

// The parameters the sources name (:q), each once.
function paramsOf(def) {
  const out = [];
  for (const s of def.sources) for (const m of s.sql.matchAll(/(^|[^:\w]):([A-Za-z_]\w*)/g)) if (!out.includes(m[2])) out.push(m[2]);
  return out;
}

// What changed from a to b, in words.
function diffDefs(a, b) {
  const out = [];
  if (!a) return ["the first version"];
  const names = (d) => d.sources.map((s) => s.name);
  for (const s of b.sources) { const was = a.sources.find((x) => x.name === s.name); if (!was) out.push(`source ${s.name} added`); else if (was.sql.trim() !== s.sql.trim()) out.push(`source ${s.name}'s query changed`); }
  for (const n of names(a)) if (!names(b).includes(n)) out.push(`source ${n} removed`);
  const hs = (d) => (d.heads?.length ? d.heads : [{ name: "score", weight: 1, calibrate: "tmm" }]);
  for (const hd of hs(b)) {
    const was = hs(a).find((x) => x.name === hd.name);
    if (!was) out.push(`head ${hd.name} added`);
    else {
      if ((was.weight ?? 1) !== (hd.weight ?? 1)) out.push(`${hd.name} weight ${n2(was.weight ?? 1)} → ${n2(hd.weight ?? 1)}`);
      if ((was.calibrate || "tmm") !== (hd.calibrate || "tmm")) out.push(`${hd.name} calibrated by ${hd.calibrate}`);
      if ((was.missing || "neutral") !== (hd.missing || "neutral")) out.push(`${hd.name} missing as ${hd.missing}`);
    }
  }
  for (const hd of hs(a)) if (!hs(b).some((x) => x.name === hd.name)) out.push(`head ${hd.name} removed`);
  if (a.k !== b.k) out.push(`k ${a.k} → ${b.k}`);
  const boost = (d, s) => (d.boost || []).filter((x) => x.source === s).reduce((t, x) => t + x.by, 0);
  for (const s of names(b)) if (boost(a, s) !== boost(b, s)) out.push(`${s} boost ${n2(boost(a, s))} → ${n2(boost(b, s))}`);
  const list = (d, key, f) => (d[key] || []).map(f).sort();
  const pairs = [["slots", (s) => `slot ${s.slot} for ${s.when.column} = ${s.when.value}`], ["quotas", (q) => quotaText(q)], ["spacing", (s) => `${s.when.value} at most ${s.at_most} in ${s.per} rows`], ["caps", (c) => `at most ${c.at_most} per ${c.per}`]];
  for (const [key, f] of pairs) {
    const x = list(a, key, f), y = list(b, key, f);
    for (const t of y) if (!x.includes(t)) out.push(t + " added");
    for (const t of x) if (!y.includes(t)) out.push(t + " removed");
  }
  const lam = (d) => d.diversify?.lambda || 0;
  if (lam(a) !== lam(b) || (a.diversify?.on || "") !== (b.diversify?.on || "")) out.push(lam(b) ? `diversity ${n2(lam(b))} on ${b.diversify.on}` : "no diversity");
  const dd = (d) => (d.dedupe ? `${d.dedupe.threshold} on ${d.dedupe.on}` : "");
  if (dd(a) !== dd(b)) out.push(b.dedupe ? `near copies dropped at cosine ${n2(b.dedupe.threshold)}` : "no dedupe");
  return out;
}

function quotaText(q) {
  const top = q.top ? ` of the top ${q.top}` : "";
  const who = q.when.column === "source" ? q.when.value : `${q.when.column} = ${q.when.value}`;
  if (q.min > 0 && q.max < 100) return `${who} ${q.min}–${q.max}%${top}`;
  if (q.min > 0) return `${who} at least ${q.min}%${top}${q.if_any ? " if any" : ""}`;
  return `${who} at most ${q.max}%${top}`;
}

function rulesSummary(d) {
  const out = [];
  if (d.slots?.length) out.push(fmt.plural(d.slots.length, "slot") + " kept");
  if (d.quotas?.length) out.push(fmt.plural(d.quotas.length, "quota"));
  if (d.spacing?.length) out.push("spacing");
  if (d.diversify?.lambda) out.push(`diversity ${n2(d.diversify.lambda)}`);
  if (d.caps?.length) out.push(d.caps.map((c) => `≤${c.at_most} per ${c.per}`).join(", "));
  if (d.dedupe) out.push("dedupe");
  return out;
}

// ------------------------------------------------------------ the page
function render(page, r) {
  const head = h("div", { class: "page-head" });
  const body = h("div", { class: "bl-body" });
  page.append(head, body);
  let alive = true, stopEditor = null;
  reshow = (route) => { if (alive) show(route); };

  function show(route) {
    r = route;
    stopEditor?.(); stopEditor = null;
    const name = r.rest?.[0] ? decodeURIComponent(r.rest[0]) : null;
    if (name) { stopEditor = editor(page, head, body, name, r.params); return; }
    list();
  }

  async function list() {
    const newBtn = h("button", { class: "btn primary", onclick: () => newBlend() }, icon("plus"), "New blend");
    gate(newBtn, "admin", "PUT /v1/blends/:name");
    clear(head, h("div", {}, h("h1", {}, "Blending"), h("p", {}, "A blend makes one list from the rows of several searches: each source's heads put on one scale, a utility, then the slate's rules (slots kept for a kind, quotas of the top rows, spacing, caps, diversity, near copies dropped). Every save is a version; SELECT ... FROM blend('name') serves it.")), h("div", { class: "tools" }, newBtn));
    clear(body, h("div", { class: "muted small" }, "Loading the blends…"));
    let blends;
    try { blends = (await api("/v1/blends")).blends || []; } catch (e) { clear(body, failure(e, "The blends")); return; }
    if (!alive) return;
    const drafts = Object.keys(DRAFTS).filter((n) => !blends.some((b) => b.name === n));
    if (!blends.length && !drafts.length) {
      clear(body, card({ body: empty("sliders", "No blends yet", "Start from two searches of your tables: their rows in one list, each kept to a share of the top rows.", newBtn.cloneNode(true)) }));
      body.querySelector(".empty .btn")?.addEventListener("click", () => newBlend());
      return;
    }
    const grid = h("div", { class: "bl-grid" });
    for (const b of blends) {
      const d = b.def;
      grid.append(h("a", { class: "bl-card", href: `#/blend/${encodeURIComponent(b.name)}` },
        h("div", { class: "row nw", style: { gap: "8px" } }, h("b", { class: "mono grow" }, b.name), h("span", { class: "small muted" }, `v${b.version}${b.versions > 1 ? ` of ${b.versions}` : ""} · ${d.k} rows`)),
        h("div", { class: "bl-sbar", "aria-hidden": "true" }, d.sources.map((s) => h("i", { style: { flexGrow: 1, background: colorOf(d, s.name) } }))),
        h("div", { class: "row wrap", style: { gap: "6px" } }, d.sources.map((s) => srcTag(d, s.name))),
        h("div", { class: "small dim" }, rulesSummary(d).join(" · ") || "no rules: best utility first"),
        h("div", { class: "small muted" }, d.note ? `“${d.note}”` : "", b.created ? ` ${b.created_by ? "by " + b.created_by + ", " : ""}${fmt.ago(Date.parse(b.created))}` : "")));
    }
    for (const n of drafts) grid.append(h("a", { class: "bl-card draft", href: `#/blend/${encodeURIComponent(n)}` }, h("b", { class: "mono" }, n), h("span", { class: "pill warn" }, "Draft, not saved"), h("div", { class: "small dim" }, fmt.plural(DRAFTS[n].sources.length, "source"))));
    clear(body, grid);
  }

  function newBlend() {
    const nameIn = h("input", { class: "input mono", placeholder: "serp", "aria-label": "The blend's name", style: { width: "100%" } });
    const sqlA = h("textarea", { class: "input mono", rows: 4, spellcheck: "false", "aria-label": "The first source's query", style: { width: "100%" }, value: "SELECT id, title, score() AS relevance FROM docs ORDER BY bm25(body, :q) DESC LIMIT 50" });
    const sqlB = h("textarea", { class: "input mono", rows: 4, spellcheck: "false", "aria-label": "The second source's query", style: { width: "100%" }, placeholder: "Another search (optional): another table, a filter, a recommender" });
    const ok = h("button", { class: "btn primary" }, "Open the draft");
    const m = modal({ title: "New blend", wide: true, body: [
      h("label", { class: "bl-field" }, h("span", { class: "small dim" }, "Name (letters, digits and _)"), nameIn),
      h("label", { class: "bl-field" }, h("span", { class: "small dim" }, "Source a: a query that ranks its own rows. Name the values rows are worth by as columns (score() AS relevance)"), sqlA),
      h("label", { class: "bl-field" }, h("span", { class: "small dim" }, "Source b"), sqlB),
      h("p", { class: "small muted", style: { margin: 0 } }, "Nothing is stored until you save the draft."),
    ], foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), ok] });
    ok.onclick = () => {
      const name = nameIn.value.trim().toLowerCase();
      if (!plain(name)) { toast("A blend's name is letters, digits and _", "bad"); return; }
      const sources = [{ name: "a", sql: sqlA.value.trim() }];
      if (sqlB.value.trim()) sources.push({ name: "b", sql: sqlB.value.trim() });
      DRAFTS[name] = { name, sources, k: 10 };
      m.close();
      location.hash = `#/blend/${encodeURIComponent(name)}`;
    };
  }

  show(r);
  return () => { alive = false; stopEditor?.(); reshow = null; };
}

// ------------------------------------------------------------ one blend
function editor(page, head, body, name, asked) {
  let alive = true;
  let saved = null, savedVersion = 0, versions = [], draft = null, last = null, seq = 0;
  // The sources' parameters, from the link (#/blend/serp?q=rust) at first.
  const params = {};
  for (const [k, v] of asked || []) params[k] = v;
  const settings = h("div", { class: "bl-settings" });
  const preview = h("div", { class: "bl-preview" });
  const status = h("span", { class: "small muted" });
  const dirtyPill = h("span", { class: "pill warn", style: { display: "none" } }, "Unsaved changes");
  const saveBtn = h("button", { class: "btn primary", onclick: () => save() }, icon("check"), "Save version");
  const discardBtn = h("button", { class: "btn", onclick: () => { draft = clone(saved || DRAFTS[name]); changed(); } }, icon("undo"), "Discard");
  gate(saveBtn, "admin", "PUT /v1/blends/:name");
  const more = h("button", { class: "btn ghost icon", title: "More", "aria-label": "More" }, icon("dots"));
  more.onclick = () => menu(more, [
    { label: "The SQL of this draft", icon: "sql", onclick: () => sqlDrawer() },
    { label: "Versions", icon: "clock", onclick: () => versionsDrawer() },
    { label: "Value on the log", icon: "recsys", onclick: () => trainDrawer() },
    "-",
    ...(saved && can("admin") ? [{ label: "Drop this blend…", icon: "trash", danger: true, onclick: () => drop() }] : []),
  ]);
  const titleEl = h("h1", { class: "mono" }, name);
  clear(head, h("div", {}, h("div", { class: "small" }, h("a", { href: "#/blend" }, "← Blending")), h("div", { class: "row nw", style: { gap: "10px" } }, titleEl, dirtyPill), status),
    h("div", { class: "tools" }, discardBtn, h("button", { class: "btn", onclick: () => versionsDrawer() }, icon("clock"), "Versions"), h("button", { class: "btn", title: "Set this draft against the serving version over the pages the blend logged, and tune it toward what people clicked", onclick: () => trainDrawer() }, icon("recsys"), "Value on the log"), saveBtn, more));
  const ed = h("div", { class: "bl-ed" }, settings, preview);
  // A slider let go, or moved by its keys: the draft runs again.
  ed.addEventListener("bl-change", () => changed());
  clear(body, ed);
  load();

  async function load() {
    try {
      const j = await api(`/v1/blends/${encodeURIComponent(name)}`);
      versions = j.versions || [];
      const cur = versions.find((v) => v.version === j.current);
      saved = cur?.def || null; savedVersion = j.current || 0;
    } catch (e) {
      if (e.status !== 404) { clear(settings, failure(e, "The blend")); return; }
      saved = null; savedVersion = 0; versions = [];
    }
    if (!alive) return;
    if (!saved && !DRAFTS[name]) { clear(settings, card({ body: empty("sliders", `No blend named ${name}`, "It may have been dropped.", h("a", { class: "btn", href: "#/blend" }, "All blends")) })); clear(preview); return; }
    draft = clone(DRAFTS[name] && !saved ? DRAFTS[name] : saved);
    for (const p of paramsOf(draft)) if (params[p] == null) params[p] = "";
    changed();
  }

  function changed(run = true) {
    const dirty = !saved || !same(draft, saved);
    dirtyPill.style.display = dirty ? "" : "none";
    dirtyPill.textContent = saved ? "Unsaved changes" : "Draft, not saved";
    discardBtn.disabled = !dirty || !saved;
    saveBtn.disabled = !dirty;
    saveBtn.replaceChildren(icon("check"), saved ? `Save as v${(versions.at(-1)?.version || savedVersion) + 1}` : "Save v1");
    status.textContent = saved ? `version ${savedVersion}${versions.length > 1 ? ` of ${versions.length}` : ""}${saved.note ? ` · “${saved.note}”` : ""}` : "not stored yet";
    if (!saved) DRAFTS[name] = draft;
    drawSettings();
    if (run) runSoon();
  }

  const runSoon = debounce(() => run(), 250);
  async function run() {
    const my = ++seq;
    const blank = paramsOf(draft).filter((n) => params[n] == null || params[n] === "");
    if (blank.length) {
      last = null;
      const focus = document.activeElement?.closest?.(".bl-preview") ? document.activeElement.getAttribute("aria-label") : null;
      clear(preview, paramsCard(), card({ body: empty("search", `Give ${blank.map((n) => ":" + n).join(" and ")} a value`, "The sources read it. The draft runs as you type, and nothing is stored.") }));
      refocus(preview, focus);
      drawSettings();
      return;
    }
    const p = {};
    for (const [k, v] of Object.entries(params)) p[k] = v !== "" && !isNaN(Number(v)) && /^-?\d/.test(v) ? Number(v) : v;
    preview.classList.add("busy");
    try {
      const j = await api("/v1/blends/preview", { method: "POST", body: { def: { ...draft, name }, params: p, costs: true } });
      if (my !== seq || !alive) return;
      last = j;
      drawPreview();
      drawSettings();
    } catch (e) {
      if (my !== seq || !alive) return;
      last = null;
      clear(preview, paramsCard(), card({ body: failure(e, "This draft's run") }));
    } finally { if (my === seq) preview.classList.remove("busy"); }
  }

  // ---------------------------------------------------------- settings
  function drawSettings() {
    if (!draft) return;
    const focus = settings.contains(document.activeElement) ? document.activeElement.getAttribute("aria-label") : null;
    clear(settings, sourcesCard(), headsCard(), slotsCard(), quotasCard(), rulesCard());
    refocus(settings, focus);
  }

  function srcStat(n) { return last?.sources?.find((s) => s.name === n); }

  function sourcesCard() {
    const rows = draft.sources.map((s, i) => {
      const st = srcStat(s.name);
      const b = (draft.boost || []).find((x) => x.source === s.name)?.by || 0;
      const boost = slider(-0.5, 0.5, 0.01, b, (v) => { draft.boost = (draft.boost || []).filter((x) => x.source !== s.name); if (Math.abs(v) > 1e-9) draft.boost.push({ source: s.name, by: Math.round(v * 100) / 100 }); }, { label: `Boost of ${s.name}`, fmtv: (v) => (v > 0 ? "+" : "") + v.toFixed(2) });
      return h("div", { class: "bl-src" },
        h("div", { class: "row nw", style: { gap: "8px" } },
          h("span", { class: "bl-sw", style: { background: colorOf(draft, s.name) }, draggable: "true", title: "Drag onto a slot to keep it for this source", ondragstart: (e) => e.dataTransfer.setData("text/blend-source", s.name) }),
          h("b", { class: "mono grow" }, s.name),
          h("span", { class: "small muted" }, st ? `${fmt.plural(st.rows, "row")} · ${fmt.ms(st.took_ms)} · ${st.placed} placed` : ""),
          h("button", { class: "btn ghost icon sm", title: "Edit its query", "aria-label": `Edit source ${s.name}`, onclick: () => editSource(i) }, icon("edit")),
          draft.sources.length > 1 ? h("button", { class: "btn ghost icon sm", title: "Remove the source", "aria-label": `Remove source ${s.name}`, onclick: () => removeSource(i) }, icon("x")) : null),
        h("div", { class: "small mono muted ellipsis", title: s.sql }, s.sql),
        h("div", { class: "row nw", style: { gap: "8px" } }, h("span", { class: "small dim", style: { width: "48px" } }, "Boost"), boost));
    });
    const add = h("button", { class: "btn sm", onclick: () => editSource(-1) }, icon("plus"), "Add a source");
    return card({ title: "Sources", sub: "each a query that ranks its own rows", body: [h("div", { class: "bl-list" }, rows), add] });
  }

  function editSource(i) {
    const s = i >= 0 ? draft.sources[i] : { name: "s" + (draft.sources.length + 1), sql: "" };
    const nameIn = h("input", { class: "input mono", value: s.name, "aria-label": "The source's name" });
    const sqlIn = h("textarea", { class: "input mono", rows: 7, spellcheck: "false", "aria-label": "The source's query", style: { width: "100%" }, value: s.sql, placeholder: "SELECT id, title, score() AS relevance FROM t ORDER BY bm25(body, :q) DESC LIMIT 50" });
    const ok = h("button", { class: "btn primary" }, i >= 0 ? "Keep" : "Add");
    const m = modal({ title: i >= 0 ? `Source ${s.name}` : "Add a source", wide: true, body: [
      h("label", { class: "bl-field" }, h("span", { class: "small dim" }, "Name: the value of the column source"), nameIn),
      h("label", { class: "bl-field" }, h("span", { class: "small dim" }, "Its query: any SELECT that ranks rows (a rank profile, rrf, recommend). Columns named like a head are that head; score is the query's own score."), sqlIn),
    ], foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), ok] });
    ok.onclick = () => {
      const n = nameIn.value.trim().toLowerCase();
      if (!plain(n)) { toast("A source's name is letters, digits and _", "bad"); return; }
      if (draft.sources.some((x, j) => j !== i && x.name === n)) { toast(`Another source is named ${n}`, "bad"); return; }
      if (!sqlIn.value.trim()) { toast("The source needs its query", "bad"); return; }
      if (i >= 0 && n !== s.name) renameSource(s.name, n);
      if (i >= 0) draft.sources[i] = { name: n, sql: sqlIn.value.trim() }; else draft.sources.push({ name: n, sql: sqlIn.value.trim() });
      for (const p of paramsOf(draft)) if (params[p] == null) params[p] = "";
      m.close(); changed();
    };
  }

  function renameSource(from, to) {
    for (const b of draft.boost || []) if (b.source === from) b.source = to;
    for (const list of [draft.slots, draft.quotas, draft.spacing]) for (const x of list || []) if (x.when.column === "source" && x.when.value === from) x.when.value = to;
  }

  function removeSource(i) {
    const n = draft.sources[i].name;
    draft.sources.splice(i, 1);
    draft.boost = (draft.boost || []).filter((b) => b.source !== n);
    const keep = (x) => !(x.when.column === "source" && x.when.value === n);
    draft.slots = (draft.slots || []).filter(keep); draft.quotas = (draft.quotas || []).filter(keep); draft.spacing = (draft.spacing || []).filter(keep);
    changed();
  }

  function numericColumns() {
    const cols = new Set();
    for (const r of [...(last?.rows || []), ...(last?.passed || [])]) for (const [c, v] of Object.entries(r.cells || {})) if (typeof v === "number") cols.add(c);
    return [...cols];
  }

  function headsCard() {
    const heads = draft.heads || [];
    const list = h("div", { class: "bl-list" });
    if (!heads.length) list.append(h("div", { class: "small dim" }, "One head, ", h("b", { class: "mono" }, "score"), ": each source's own score, scaled by tmm so that each source's best row is 1. Add heads to weigh rows by more than one value."));
    heads.forEach((hd, i) => {
      list.append(h("div", { class: "bl-head" },
        h("div", { class: "row nw", style: { gap: "8px" } },
          h("i", { class: "bl-shade", style: { opacity: HEAD_SHADES[i % HEAD_SHADES.length] } }),
          h("b", { class: "mono grow" }, hd.name),
          seg(CALS, hd.calibrate || "tmm", (v) => { hd.calibrate = v; changed(); }, { "aria-label": `Calibration of ${hd.name}` }),
          h("button", { class: "btn ghost icon sm", title: "Remove the head", "aria-label": `Remove head ${hd.name}`, onclick: () => { heads.splice(i, 1); draft.heads = heads; changed(); } }, icon("x"))),
        h("div", { class: "row nw", style: { gap: "8px" } }, h("span", { class: "small dim", style: { width: "48px" } }, "Weight"),
          slider(-1, 2, 0.05, hd.weight ?? 1, (v) => { hd.weight = Math.round(v * 100) / 100; }, { label: `Weight of ${hd.name}` }),
          seg([["neutral", "neutral", "A source without it gives its rows the others' mean"], ["zero", "zero", "A source without it gives its rows 0"]], hd.missing || "neutral", (v) => { hd.missing = v; changed(); }, { "aria-label": `Missing ${hd.name}` }))));
    });
    const add = h("button", { class: "btn sm" }, icon("plus"), "Add a head");
    add.onclick = () => {
      const have = new Set(heads.map((x) => x.name));
      const cols = ["score", ...numericColumns()].filter((c) => !have.has(c.toLowerCase()));
      menu(add, [...cols.map((c) => ({ label: c, onclick: () => addHead(c) })), "-", { label: "Another column…", icon: "edit", onclick: () => askText("Add a head", "The column the sources name (each source that lacks it counts as MISSING says)").then((n) => n && addHead(n)) }]);
    };
    const addHead = (c) => {
      const n = c.trim().toLowerCase();
      if (!plain(n)) { toast("A head is a column's name", "bad"); return; }
      if (!draft.heads?.length) draft.heads = [];
      if (!draft.heads.some((x) => x.name === n)) draft.heads.push({ name: n, weight: 1, calibrate: "tmm", missing: "neutral" });
      changed();
    };
    const terms = (heads.length ? heads : [{ name: "score", weight: 1 }]).map((x) => `${n2(x.weight ?? 1)} × ${x.name}`).join(" + ");
    return card({ title: "Heads and utility", sub: "what a row is worth", body: [list, h("div", { class: "bl-formula mono small" }, `utility = ${terms}${draft.boost?.length ? " + the source's boost" : ""}`), add] });
  }

  function slotsCard() {
    const k = draft.k || 10;
    const strip = h("div", { class: "bl-slots", role: "list" });
    for (let i = 1; i <= Math.min(k, 30); i++) {
      const pin = (draft.slots || []).find((s) => s.slot === i);
      const row = last?.rows?.find((x) => x.slot === i);
      const cell = h("button", { class: "bl-slot" + (pin ? " pinned" : ""), role: "listitem", title: pin ? `Slot ${i} is kept for ${pin.when.column} = ${pin.when.value}: click to free it` : `Slot ${i}${row ? `: ${row.source} now` : ""}. Drop a source here, or click, to keep it for that source`, style: pin && pin.when.column === "source" ? { borderColor: colorOf(draft, pin.when.value) } : {} },
        h("span", { class: "n" }, String(i)), h("i", { style: { background: row ? colorOf(draft, row.source) : "transparent" } }), pin ? icon("pin") : null);
      cell.ondragover = (e) => { if ([...e.dataTransfer.types].includes("text/blend-source")) { e.preventDefault(); cell.classList.add("over"); } };
      cell.ondragleave = () => cell.classList.remove("over");
      cell.ondrop = (e) => { e.preventDefault(); cell.classList.remove("over"); pinSlot(i, e.dataTransfer.getData("text/blend-source")); };
      cell.onclick = () => {
        if (pin) { draft.slots = draft.slots.filter((s) => s !== pin); changed(); return; }
        menu(cell, draft.sources.map((s) => ({ label: `Keep slot ${i} for ${s.name}`, color: colorOf(draft, s.name), onclick: () => pinSlot(i, s.name) })));
      };
      strip.append(cell);
    }
    const kIn = stepper(1, 100, k, (v) => { draft.k = v; draft.slots = (draft.slots || []).filter((s) => s.slot <= v); for (const q of draft.quotas || []) if (q.top && q.top > v) q.top = v; changed(); }, "Rows placed");
    const chips = h("div", { class: "row wrap", style: { gap: "6px" } }, draft.sources.map((s) => { const t = srcTag(draft, s.name); t.draggable = true; t.classList.add("drag"); t.ondragstart = (e) => e.dataTransfer.setData("text/blend-source", s.name); return t; }));
    return card({ title: "Slots", sub: "drag a source onto a slot to keep it", tools: h("label", { class: "row nw small dim", style: { gap: "6px" } }, "Rows", kIn), body: [strip, chips] });
  }

  function pinSlot(i, src) {
    if (!draft.sources.some((s) => s.name === src)) return;
    draft.slots = (draft.slots || []).filter((s) => s.slot !== i);
    draft.slots.push({ slot: i, when: { column: "source", value: src } });
    draft.slots.sort((a, b) => a.slot - b.slot);
    changed();
  }

  function quotasCard() {
    const k = draft.k || 10;
    const quotas = draft.quotas || [];
    const list = h("div", { class: "bl-list" });
    quotas.forEach((q, i) => {
      const met = last?.quotas?.[i];
      const n = Math.min(q.top || k, k);
      const label = h("span", { class: "small" });
      const sayIt = () => { label.textContent = `${q.min}%–${q.max}% of the top ${n}`; };
      sayIt();
      const now = met ? met.got / Math.max(1, met.top) : null;
      const track = rangeQuota(q, n, now, (done) => { sayIt(); if (done) changed(); });
      const topIn = stepper(1, k, n, (v) => { q.top = v >= k ? undefined : v; changed(); }, "Over the top rows");
      list.append(h("div", { class: "bl-quota" },
        h("div", { class: "row nw", style: { gap: "8px" } },
          q.when.column === "source" ? srcTag(draft, q.when.value) : h("span", { class: "bl-tag mono" }, `${q.when.column} = ${q.when.value}`),
          label, h("span", { class: "grow" }),
          met ? h("span", { class: "small " + (met.idle ? "muted" : met.met ? "good" : "bad"), title: met.idle ? "No row of its kind: it holds only if any" : "" }, met.idle ? "idle" : `${met.got} of ${met.top} now${met.met ? "" : ": not met"}`) : null,
          h("button", { class: "btn ghost icon sm", "aria-label": "Remove the quota", title: "Remove the quota", onclick: () => { quotas.splice(i, 1); draft.quotas = quotas; changed(); } }, icon("x"))),
        track,
        h("div", { class: "row nw small dim", style: { gap: "10px" } }, h("label", { class: "row nw", style: { gap: "6px" } }, "of the top", topIn),
          q.min > 0 ? h("label", { class: "row nw", style: { gap: "6px" }, title: "The least share holds only when some row is of its kind" }, h("input", { type: "checkbox", checked: !!q.if_any, onchange: (e) => { q.if_any = e.target.checked || undefined; changed(); } }), "only if any") : null)));
    });
    const add = h("button", { class: "btn sm" }, icon("plus"), "Add a quota");
    add.onclick = () => menu(add, draft.sources.map((s) => ({ label: `A share of ${s.name}`, color: colorOf(draft, s.name), onclick: () => { draft.quotas = [...quotas, { when: { column: "source", value: s.name }, min: 0, max: 100 }]; changed(); } })));
    if (!quotas.length) list.append(h("div", { class: "small dim" }, "No quotas: each source gets the share its utility earns. A quota holds a source between two shares of the top rows."));
    return card({ title: "Quotas", sub: "shares of the top rows", body: [list, add] });
  }

  function rulesCard() {
    const cols = last?.columns || [];
    const vecs = last?.vectors || [];
    const colSel = (value, onChange, prefer) => {
      const opts = [...new Set([...(prefer || []), ...cols, value].filter(Boolean))];
      const sel = h("select", { class: "input sm", onchange: () => onChange(sel.value) }, opts.map((c) => h("option", { value: c, selected: c === value }, c)));
      return sel;
    };
    const parts = [];
    // Spacing
    for (const [i, sp] of (draft.spacing || []).entries()) {
      parts.push(h("div", { class: "bl-rule" }, h("span", { class: "small dim" }, "Spacing"),
        h("select", { class: "input sm", "aria-label": "Source", onchange: (e) => { sp.when = { column: "source", value: e.target.value }; changed(); } }, draft.sources.map((s) => h("option", { value: s.name, selected: sp.when.value === s.name }, s.name))),
        h("span", { class: "small" }, "at most"), stepper(0, Math.max(0, sp.per - 1), sp.at_most, (v) => { sp.at_most = v; changed(); }, "At most"),
        h("span", { class: "small" }, "in any"), stepper(2, 50, sp.per, (v) => { sp.per = v; sp.at_most = Math.min(sp.at_most, v - 1); changed(); }, "Rows"), h("span", { class: "small" }, "rows"),
        h("button", { class: "btn ghost icon sm", "aria-label": "Remove", onclick: () => { draft.spacing.splice(i, 1); changed(); } }, icon("x"))));
    }
    // Caps
    for (const [i, c] of (draft.caps || []).entries()) {
      parts.push(h("div", { class: "bl-rule" }, h("span", { class: "small dim" }, "Cap"), h("span", { class: "small" }, "at most"), stepper(1, 50, c.at_most, (v) => { c.at_most = v; changed(); }, "At most"), h("span", { class: "small" }, "per"), colSel(c.per, (v) => { c.per = v; changed(); }),
        h("button", { class: "btn ghost icon sm", "aria-label": "Remove", onclick: () => { draft.caps.splice(i, 1); changed(); } }, icon("x"))));
    }
    // Diversity
    const lam = draft.diversify?.lambda || 0;
    parts.push(h("div", { class: "bl-rule" }, h("span", { class: "small dim" }, "Diversity"),
      slider(0, 1, 0.05, lam, (v) => { const on = draft.diversify?.on || vecs[0] || cols[0] || "id"; draft.diversify = v > 0 ? { lambda: Math.round(v * 100) / 100, on } : undefined; }, { label: "Diversity", fmtv: (v) => (v > 0 ? v.toFixed(2) : "off") }),
      lam > 0 ? h("span", { class: "small" }, "on") : null, lam > 0 ? colSel(draft.diversify.on, (v) => { draft.diversify.on = v; changed(); }, vecs) : null));
    // Dedupe
    const dd = draft.dedupe;
    parts.push(h("div", { class: "bl-rule" }, h("span", { class: "small dim" }, "Near copies"),
      h("label", { class: "row nw small", style: { gap: "6px" } }, h("input", { type: "checkbox", checked: !!dd, disabled: !vecs.length && !dd, onchange: (e) => { draft.dedupe = e.target.checked ? { threshold: 0.95, on: vecs[0] || "emb" } : undefined; changed(); } }), dd ? "dropped at cosine" : vecs.length ? "kept" : "need a vector column in the sources"),
      dd ? slider(0.8, 1, 0.005, dd.threshold, (v) => { dd.threshold = Math.round(v * 1000) / 1000; }, { label: "Cosine", fmtv: (v) => v.toFixed(3) }) : null,
      dd ? colSel(dd.on, (v) => { dd.on = v; changed(); }, vecs) : null));
    const add = h("button", { class: "btn sm" }, icon("plus"), "Add a rule");
    add.onclick = () => menu(add, [
      { label: "Spacing: at most n of a source in any m rows", onclick: () => { draft.spacing = [...(draft.spacing || []), { when: { column: "source", value: draft.sources.at(-1).name }, at_most: 1, per: 3 }]; changed(); } },
      { label: "Cap: at most n rows per value of a column", onclick: () => { draft.caps = [...(draft.caps || []), { at_most: 2, per: cols.find((c) => /author|site|domain|user|owner/i.test(c)) || cols[0] || "id" }]; changed(); } },
    ]);
    return card({ title: "The slate's rules", sub: "spacing, caps, diversity, near copies", body: [h("div", { class: "bl-list" }, parts), add] });
  }

  // ---------------------------------------------------------- preview
  function paramsCard() {
    const names = Object.keys(params).filter((p) => paramsOf(draft).includes(p));
    const inputs = names.map((p) => h("label", { class: "row nw", style: { gap: "6px" } }, h("span", { class: "mono small dim" }, ":" + p),
      h("input", { class: "input", value: params[p], placeholder: p === "q" ? "a search" : "a value", "aria-label": `Parameter ${p}`, oninput: (e) => { params[p] = e.target.value; runSoon(); } })));
    return card({ title: "Try it", sub: names.length ? "the sources' parameters" : "the sources take no parameters", tools: h("button", { class: "btn sm", onclick: () => run() }, icon("refresh"), "Run"), body: h("div", { class: "row wrap", style: { gap: "10px" } }, inputs.length ? inputs : h("span", { class: "small muted" }, "Each change runs the draft again; nothing is stored.")) });
  }

  function drawPreview() {
    const j = last;
    const focus = document.activeElement?.closest?.(".bl-preview") ? document.activeElement.getAttribute("aria-label") : null;
    const shares = j.sources.filter((s) => s.placed > 0);
    const total = Math.max(1, j.rows.length);
    const maxU = Math.max(1e-9, ...j.rows.map((r) => Object.values(r.parts).reduce((t, v) => t + Math.max(0, v), 0) + Math.max(0, r.boost)));
    const titleCol = TITLE_COLS.find((c) => j.columns.some((x) => x.toLowerCase() === c));
    const titleOf = (r) => { const c = titleCol && Object.keys(r.cells).find((x) => x.toLowerCase() === titleCol); const v = c ? r.cells[c] : null; return v != null ? String(v) : String(r.key ?? ""); };
    const kindPill = (r) => r.kind === "pin" ? h("span", { class: "bl-kind pin", title: r.why }, icon("pin"), "slot") : r.kind === "quota" ? h("span", { class: "bl-kind quota", title: r.why }, "quota") : r.kind === "relaxed" ? h("span", { class: "bl-kind relaxed", title: r.why }, icon("warn"), "rule set aside") : null;
    // The parts in the heads' order, the order of their shades in the key.
    const heads = (j.heads || []).map((hd) => hd.name);
    const partsOf = (r) => heads.map((n) => [n, r.parts[n] ?? 0]);
    const ubar = (r) => h("div", { class: "bl-ubar", title: [...partsOf(r).map(([k, v]) => `${k} ${v.toFixed(3)}`), r.boost ? `boost ${r.boost.toFixed(3)}` : null].filter(Boolean).join(" · ") },
      partsOf(r).map(([, v], i) => h("i", { style: { width: (Math.max(0, v) / maxU) * 100 + "%", background: colorOf(draft, r.source), opacity: HEAD_SHADES[i % HEAD_SHADES.length] } })),
      r.boost > 0 ? h("i", { class: "boost", style: { width: (r.boost / maxU) * 100 + "%" } }) : null);
    const slate = h("ol", { class: "bl-slate" }, j.rows.map((r) => h("li", { class: "bl-row" },
      h("span", { class: "slot" }, String(r.slot)),
      h("div", { class: "grow", style: { minWidth: 0 } },
        h("div", { class: "row nw", style: { gap: "8px" } }, srcTag(draft, r.source, r.also?.length ? h("span", { class: "muted" }, ` +${r.also.join(", ")}`) : null), h("span", { class: "ellipsis grow", title: titleOf(r) }, titleOf(r)), kindPill(r)),
        h("div", { class: "row nw", style: { gap: "8px" } }, ubar(r), h("span", { class: "small mono num" }, n2(r.utility)), r.adjusted < r.utility - 1e-9 ? h("span", { class: "small muted mono", title: "after diversity" }, `→ ${n2(r.adjusted)}`) : null),
        h("div", { class: "small muted ellipsis", title: r.why }, `#${r.rank} in ${r.source} · ${r.why}`)))));
    const quotaRows = j.quotas.map((q) => h("div", { class: "row nw small", style: { gap: "8px" } }, icon(q.idle ? "minus" : q.met ? "check" : "warn"), h("span", { class: "mono grow ellipsis" }, q.rule), h("span", { class: q.idle ? "muted" : q.met ? "good" : "bad" }, q.idle ? "idle" : `${q.got} of ${q.top} (needs ${q.least}–${q.most})`)));
    const maxCost = Math.max(0.001, ...j.costs.map((c) => c.share));
    const costRows = j.costs.map((c) => h("div", { class: "bl-cost" }, h("span", { class: "mono small ellipsis", title: c.rule }, c.rule), h("div", { class: "barbox" }, h("div", { class: "bar", style: { width: (c.share / maxCost) * 100 + "%" } })), h("span", { class: "small num" }, `${(c.share * 100).toFixed(1)}%`), h("span", { class: "small muted num" }, fmt.plural(c.moved, "row") + " moved")));
    const passed = j.passed.map((r) => h("div", { class: "bl-passed" }, srcTag(draft, r.source), h("span", { class: "ellipsis grow", title: titleOf(r) }, titleOf(r)), h("span", { class: "small mono" }, n2(r.utility)), h("span", { class: "small muted ellipsis", title: r.why, style: { maxWidth: "45%" } }, r.why)));
    clear(preview,
      paramsCard(),
      card({ title: "The slate", sub: `${j.rows.length} rows · sources ${fmt.ms(Math.max(...j.sources.map((s) => s.took_ms), 0))} · fill ${fmt.ms(j.fill_ms)}`, body: [
        h("div", { class: "bl-sbar big", title: shares.map((s) => `${s.name} ${s.placed}`).join(" · ") }, shares.map((s) => h("i", { style: { flexGrow: s.placed, background: colorOf(draft, s.name) } }))),
        h("div", { class: "row wrap small", style: { gap: "10px" } }, j.sources.map((s) => h("span", { class: "row nw", style: { gap: "4px" } }, h("i", { class: "bl-dot", style: { background: colorOf(draft, s.name) } }), `${s.name} ${s.placed} of ${total} (${fmt.pct(s.placed / total)})`))),
        h("div", { class: "row wrap small muted", style: { gap: "10px" } }, (j.heads || []).map((hd, i) => h("span", { class: "row nw", style: { gap: "4px" } }, h("i", { class: "bl-dot", style: { background: "var(--text-2)", opacity: HEAD_SHADES[i % HEAD_SHADES.length] } }), `${hd.name} ×${n2(hd.weight)} (${hd.calibrate})`))),
        j.rows.length ? slate : empty("info", "No rows", "The sources found nothing for these parameters."),
      ] }),
      j.quotas.length ? card({ title: "Quotas", sub: "as this slate met them", body: quotaRows }) : null,
      j.costs.length ? card({ title: "What each rule costs", sub: "the share of the slate's discounted utility it gives up", body: costRows }) : null,
      passed.length ? card({ title: "Passed over", sub: "rows worth more than one placed, and why", body: passed }) : null,
      h("details", { class: "bl-plan" }, h("summary", { class: "small muted" }, "Plan lines"), sqlBlock(j.plan.join("\n"))));
    refocus(preview, focus);
  }

  // ---------------------------------------------------------- versions, SQL, save
  function sqlDrawer() {
    const sql = last?.sql;
    drawer({ title: "The SQL of this draft", sub: "CREATE OR REPLACE BLEND writes it as the next version", body: sql ? [sqlBlock(sql), h("p", { class: "small muted" }, `Serve it with: SELECT slot, source, ${TITLE_COLS.find((c) => last.columns.includes(c)) || last.columns[0] || "id"}, utility, blend_decision FROM blend('${name}'${paramsOf(draft).map((p) => `, ${p} => :${p}`).join("")});`)] : h("p", { class: "muted" }, "Run the draft first."), foot: sql ? [h("button", { class: "btn", onclick: () => copy(sql, "The SQL") }, icon("copy"), "Copy")] : null, wide: true });
  }

  async function versionsDrawer() {
    try { const j = await api(`/v1/blends/${encodeURIComponent(name)}`); versions = j.versions || []; } catch (e) { if (e.status !== 404) { toast(e.message, "bad"); return; } versions = []; }
    const rows = versions.slice().reverse().map((v, i, all) => {
      const prev = all[i + 1];
      const changes = v.dropped ? ["dropped"] : diffDefs(prev && !prev.dropped ? prev.def : null, v.def);
      return h("div", { class: "bl-version" + (v.version === savedVersion ? " current" : "") },
        h("div", { class: "row nw", style: { gap: "8px" } }, h("b", { class: "mono" }, `v${v.version}`), v.version === savedVersion ? h("span", { class: "pill ok" }, "serving") : null, h("span", { class: "grow small muted" }, `${v.created_by ? v.created_by + ", " : ""}${v.created ? fmt.ago(Date.parse(v.created)) : ""}`),
          !v.dropped ? h("button", { class: "btn sm", onclick: () => { draft = clone(v.def); d.close(); changed(); toast(`v${v.version} is in the editor: save it to serve it again`); } }, "Open") : null),
        v.note ? h("div", { class: "small" }, `“${v.note}”`) : null,
        h("ul", { class: "small dim" }, changes.slice(0, 8).map((c) => h("li", {}, c)), changes.length > 8 ? h("li", {}, `and ${changes.length - 8} more`) : null));
    });
    const d = drawer({ title: `Versions of ${name}`, sub: "each save is one; blend('" + name + "@n') reads version n", body: rows.length ? rows : empty("clock", "No versions yet", "Save the draft to make version 1.") });
  }

  async function save() {
    const changes = diffDefs(saved, draft);
    const noteIn = h("textarea", { class: "input", rows: 2, style: { width: "100%" }, "aria-label": "What this version changes", value: draft.note && saved && draft.note !== saved.note ? draft.note : changes.slice(0, 3).join("; ") });
    const ok = h("button", { class: "btn primary" }, saved ? `Save as the next version` : "Save version 1");
    const m = modal({ title: `Save ${name}`, body: [h("ul", { class: "small dim" }, changes.map((c) => h("li", {}, c))), h("label", { class: "bl-field" }, h("span", { class: "small dim" }, "What it changes, kept with the version"), noteIn), h("p", { class: "small muted", style: { margin: 0 } }, "Queries of blend('" + name + "') serve the new version at once.")], foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), ok] });
    ok.onclick = async () => {
      ok.disabled = true;
      try {
        const def = { ...clone(draft), name, note: noteIn.value.trim() || undefined };
        const j = await api(`/v1/blends/${encodeURIComponent(name)}`, { method: "PUT", body: { def } });
        m.close();
        delete DRAFTS[name];
        toast(`Saved ${name} v${j.version}`, "ok");
        await load();
      } catch (e) { ok.disabled = false; toast(e.message, "bad", 8000); }
    };
  }

  // ---------------------------------------------------------- valued on the log
  async function trainDrawer() {
    let ts = [];
    try { ts = await tables(true); } catch (e) { toast(e.message, "bad"); return; }
    const logs = ts.filter(isLog);
    const was = store.get("blend.train", null);
    const st = was && logs.some((t) => t.table === was.log) ? was : { log: logs[0]?.table || "rank_log", events: guessEvents(ts, logs[0]?.table || "rank_log"), window: "1 hour", discount: false };
    const keep = () => store.set("blend.train", st);
    const ctl = h("div", { class: "bt-ctl" });
    const out = h("div", { class: "bt-out" });
    const tuneOut = h("div", { class: "bt-tune" });
    const d = drawer({ title: `Value ${name} on the log`, sub: "what people clicked on its exploring pages, reweighted by each slot's chance", body: [ctl, out, tuneOut], wide: true });
    let my = 0, pick = 0, est = null;
    const tableOf = (n) => ts.find((t) => t.table === n);
    const exportBody = () => {
      const e = st.events;
      const x = { table: st.log, window: st.window };
      if (e?.table && e.user && e.item && e.time) x.events = { table: e.table, user: e.user, item: e.item, time: e.time, ...(e.action ? { action: e.action } : {}) };
      return x;
    };
    const ready = () => !!exportBody().events;
    const dirty = () => !saved || !same(draft, saved);
    const targets = () => [...(saved ? [`${name}@${savedVersion}`] : []), ...(dirty() ? [{ def: { ...clone(draft), name }, label: "this draft" }] : [])];
    drawCtl();
    if (!logs.length) { howTo(out, "No rank log yet", `A query of blend('${name}') run with "log" writes each page it served, every row of every source, and each slot's chance when it explores.`); return; }
    evaluate();

    function drawCtl() {
      const ev = st.events || {};
      const et = tableOf(ev.table);
      const fsel = (key, types, none) => h("select", { class: "select sm mono", "aria-label": `The events' ${key}`, onchange: (e) => { st.events = { ...ev, [key]: e.target.value }; keep(); drawCtl(); evaluate(); } },
        none ? h("option", { value: "" }, "(none)") : null,
        (et?.short?.fields || []).filter((f) => types.includes(f.type) && !f.multi).map((f) => h("option", { value: f.name, selected: f.name === ev[key] }, f.name)));
      clear(ctl,
        h("div", { class: "bt-row" },
          h("label", { class: "bt-lbl" }, "log", h("select", { class: "select sm", "aria-label": "The log", onchange: (e) => { st.log = e.target.value; keep(); evaluate(); } }, (logs.length ? logs : [{ table: st.log }]).map((t) => h("option", { value: t.table, selected: t.table === st.log }, t.table)))),
          h("label", { class: "bt-lbl" }, "clicks in", h("select", { class: "select sm", "aria-label": "The events", onchange: (e) => { const t = tableOf(e.target.value); st.events = t ? guessEvents([t], st.log) || { table: t.table, user: "", item: "", time: "", action: "" } : null; keep(); drawCtl(); evaluate(); } },
            h("option", { value: "" }, "(choose the events)"), ts.filter((t) => !isLog(t)).map((t) => h("option", { value: t.table, selected: t.table === ev.table }, t.table)))),
          et ? h("span", { class: "bt-map", title: "Which columns of the events are the user (the log's context), the item (a row's key) and the time" }, h("span", { class: "muted" }, "user"), fsel("user", ["keyword", "int"]), h("span", { class: "muted" }, "item"), fsel("item", ["keyword", "int"]), h("span", { class: "muted" }, "time"), fsel("time", ["timestamp", "int"]), h("span", { class: "muted" }, "action"), fsel("action", ["keyword"], true)) : null),
        h("div", { class: "bt-row" },
          h("label", { class: "bt-lbl" }, "a click counts within", seg([["5 minutes", "5 min"], ["30 minutes", "30 min"], ["1 hour", "1 h"], ["1 day", "1 day"]], st.window, (v) => { st.window = v; keep(); evaluate(); })),
          h("label", { class: "bt-lbl" }, "a page is worth", seg([[false, "its clicks"], [true, "its clicks, discounted by slot"]], st.discount, (v) => { st.discount = v; keep(); evaluate(); })),
          h("span", { class: "grow" }),
          h("button", { class: "btn sm", onclick: () => evaluate() }, icon("refresh"), "Value again"),
          h("button", { class: "btn sm primary", disabled: !saved, title: saved ? "Blends near this draft, tried on half the searches and proven on the other half" : "Save the blend and log it first", onclick: () => tune() }, icon("sparkle"), "Tune toward clicks")));
    }

    async function evaluate() {
      const me = ++my;
      clear(tuneOut);
      if (!saved) { clear(out, card({ body: empty("recsys", "Save the blend first", `Its pages are logged as blend('${name}'): save a version, serve it with "log", and come back.`) })); return; }
      if (!ready()) { clear(out, card({ body: empty("recsys", "Which table holds the clicks?", "Pick the events and which of their columns are the user, the item and the time. A row's label is a click by the page's user on it within the window.") })); return; }
      out.classList.add("busy");
      try {
        const j = await api(`/v1/blends/${encodeURIComponent(name)}/evaluate`, { method: "POST", body: { export: exportBody(), targets: targets(), discount: st.discount } });
        if (me !== my) return;
        est = j;
        drawOut();
      } catch (e) { if (me === my) clear(out, card({ body: failure(e, "Valuing on the log") })); }
      finally { if (me === my) out.classList.remove("busy"); }
    }

    function drawOut() {
      const g = est.gathered;
      if (!g.requests) {
        const why = g.not_exploring ? `${fmt.n(g.not_exploring)} of its searches were logged without exploring: a page that always shows the best rows says nothing of the others.` : `${st.log} has no page of blend('${name}')${g.other ? ` (${fmt.n(g.other)} pages of other queries)` : ""}.`;
        howTo(out, "No exploring pages of this blend yet", why);
        return;
      }
      const E = est.estimates;
      const reads = est.model?.reads || [];
      pick = Math.min(pick, E.length - 1);
      clear(out,
        h("div", { class: "small muted" }, `${fmt.n(g.requests)} exploring searches${g.not_exploring ? `, ${fmt.n(g.not_exploring)} more logged without exploring` : ""}. The click model behind cascade doubly robust reads the slot and ${reads.length ? reads.join(", ") : "no numbers"}, fitted on one half of the searches to value the other.`),
        h("div", { class: "bt-verdicts" }, E.map((e) => verdict(e))),
        card({ title: "What a search would earn", sub: `${st.discount ? "clicks discounted by slot" : "clicks"} per search, each with its 95% interval`, body: forest(E) }),
        card({ title: "Slot by slot", sub: "the value each slot adds, and the searches behind it", tools: E.length > 1 ? seg(E.map((e, i) => [i, e.label]), pick, (v) => { pick = v; drawOut(); }) : null, body: slots(E[pick]) }));
    }

    function verdict(e) {
      const cls = { better: "ok", worse: "bad", same: "", "not proven": "warn" }[e.verdict] ?? "";
      const lift = e.lift?.value;
      return h("div", { class: "bt-verdict" },
        h("div", { class: "row nw", style: { gap: "8px" } }, h("b", { class: "mono" }, e.label), h("span", { class: "pill " + cls }, e.verdict), h("span", { class: "grow" }), h("span", { class: "mono" }, `${num(e.cdr.value)} ± ${num(1.96 * e.cdr.se)}`)),
        lift != null ? h("div", { class: "small" }, `${lift >= 0 ? "+" : "−"}${num(Math.abs(lift))} ± ${num(1.96 * e.lift.se)} a search against ${e.against}`) : null,
        h("div", { class: "small muted" }, e.verdict === "not proven" ? e.why : e.why.replace(/^no difference shown against [^:]+: /, "")),
        h("div", { class: "bt-facts small dim" },
          fact("support", `${fmt.n(e.support)} effective searches at slot 1`, e.trusted ? "" : "warn"),
          fact("coverage", fmt.pct(e.coverage), e.coverage < 0.95 ? "warn" : ""),
          e.switched > 0.005 ? fact("model alone", `below a slot on ${fmt.pct(e.switched)} of pages`, "") : null,
          fact("SNIPS checks", `the top ${e.check_depth} slot${e.check_depth === 1 ? "" : "s"}`, e.check_depth ? "" : "warn")));
    }

    // Each blend's value with its interval, the logged blend's beside it;
    // and over the top slots SNIPS sees well, the two estimators side by side.
    function forest(E) {
      const rows = [{ label: "as logged", parts: [["logged", E[0].logged, "lg"]] }, ...E.map((e) => ({ label: e.label, parts: [["cascade doubly robust", e.cdr, "cdr"]] }))];
      const check = E.map((e) => ({ label: e.label, parts: [["cascade doubly robust", e.cdr_top, "cdr"], ["SNIPS", e.snips, "sn"]], depth: e.check_depth }));
      return [plot(rows, "the whole page"), h("div", { class: "small muted", style: { marginTop: "6px" } }, "The check: over the top slots each has the searches for, SNIPS (no model, ○) should agree with cascade doubly robust (●). Far apart, the click model is off."), plot(check, null)];
    }

    function plot(rows, caption) {
      const narrow = innerWidth < 700;
      const W = narrow ? 380 : 640, L = narrow ? 110 : 150, R = 16, rh = 26, H = rows.length * rh + 28;
      const all = rows.flatMap((r) => r.parts.flatMap(([, iv]) => [iv.value - 1.96 * iv.se, iv.value + 1.96 * iv.se])).filter(Number.isFinite);
      let lo = Math.min(...all), hi = Math.max(...all);
      if (!(hi > lo)) { lo -= 0.5; hi += 0.5; }
      const pad = (hi - lo) * 0.08; lo -= pad; hi += pad;
      const x = (v) => L + ((v - lo) / (hi - lo)) * (W - L - R);
      const g = svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "bt-plot", role: "img", "aria-label": caption ? `Values of ${caption}` : "SNIPS beside cascade doubly robust" });
      const step = niceTick(hi - lo);
      for (let t = Math.ceil(lo / step) * step; t <= hi; t += step) {
        g.append(svg("line", { x1: x(t), x2: x(t), y1: 4, y2: H - 22, class: "bt-grid" }), svg("text", { x: x(t), y: H - 8, class: "bt-tick", "text-anchor": "middle" }, num(t)));
      }
      rows.forEach((r, i) => {
        const y = 4 + i * rh + rh / 2;
        g.append(svg("text", { x: L - 10, y: y + 4, class: "bt-name", "text-anchor": "end" }, r.label + (r.depth != null ? ` · top ${r.depth}` : "")));
        r.parts.forEach(([what, iv, cls], k) => {
          if (!Number.isFinite(iv.value)) return;
          const yy = y + (r.parts.length > 1 ? (k - 0.5) * 9 : 0);
          const a = x(iv.value - 1.96 * iv.se), b = x(iv.value + 1.96 * iv.se);
          const tip = `${what}: ${num(iv.value)} (${num(iv.value - 1.96 * iv.se)} to ${num(iv.value + 1.96 * iv.se)})`;
          g.append(svg("g", { class: "bt-iv " + cls }, svg("title", {}, tip), svg("line", { x1: a, x2: b, y1: yy, y2: yy }), svg("line", { x1: a, x2: a, y1: yy - 4, y2: yy + 4 }), svg("line", { x1: b, x2: b, y1: yy - 4, y2: yy + 4 }),
            cls === "lg" ? svg("path", { d: `M${x(iv.value)} ${yy - 5}l5 5-5 5-5-5z` }) : svg("circle", { cx: x(iv.value), cy: yy, r: 4.5 })));
        });
      });
      return g;
    }

    // Per slot: the value it adds (cascade doubly robust ●, SNIPS ○, the
    // logged blend ◆), and under it the searches behind it, shaded where
    // the logging blend could not have drawn the target's page.
    function slots(e) {
      const S = e.slots || [];
      if (!S.length) return empty("recsys", "No slots valued");
      const W = innerWidth < 700 ? 380 : 640, L = 40, R = 12, top = 150, strip = 46, H = top + strip + 26;
      const vals = S.flatMap((x) => [x.cdr, x.snips, x.logged]).filter(Number.isFinite);
      const hi = Math.max(1e-9, ...vals) * 1.12, lo = Math.min(0, ...vals);
      const cw = (W - L - R) / S.length;
      const cx = (i) => L + cw * (i + 0.5);
      const y = (v) => 8 + (1 - (v - lo) / (hi - lo)) * (top - 16);
      const g = svg("svg", { viewBox: `0 0 ${W} ${H}`, class: "bt-plot", role: "img", "aria-label": `Slot by slot values of ${e.label}` });
      const step = niceTick(hi - lo);
      for (let t = Math.ceil(lo / step) * step; t <= hi; t += step) g.append(svg("line", { x1: L, x2: W - R, y1: y(t), y2: y(t), class: "bt-grid" }), svg("text", { x: L - 6, y: y(t) + 4, class: "bt-tick", "text-anchor": "end" }, num(t)));
      const line = (key, cls) => svg("path", { class: "bt-line " + cls, d: S.map((x, i) => (Number.isFinite(x[key]) ? `${i ? "L" : "M"}${cx(i)} ${y(x[key])}` : "")).join("") });
      g.append(line("logged", "lg"), line("cdr", "cdr"));
      S.forEach((x, i) => {
        const tip = `slot ${x.slot}: cascade doubly robust ${num(x.cdr)}, SNIPS ${num(x.snips)}, model ${num(x.model)}, logged ${num(x.logged)}; ${fmt.n(x.ess)} effective searches; ${fmt.pct(x.coverage)} of the pages the logging blend could draw`;
        const col = svg("g", {}, svg("title", {}, tip));
        if (Number.isFinite(x.logged)) col.append(svg("path", { class: "bt-pt lg", d: `M${cx(i)} ${y(x.logged) - 5}l5 5-5 5-5-5z` }));
        if (Number.isFinite(x.snips) && i < e.check_depth) col.append(svg("circle", { class: "bt-pt sn", cx: cx(i), cy: y(x.snips), r: 4.5 }));
        if (Number.isFinite(x.cdr)) col.append(svg("circle", { class: "bt-pt cdr", cx: cx(i), cy: y(x.cdr), r: 4.5 }));
        const share = Math.max(0, Math.min(1, x.ess / Math.max(1, e.requests)));
        const bh = Math.max(1.5, share * (strip - 10));
        col.append(svg("rect", { class: "bt-ess" + (x.coverage < 0.95 ? " thin" : ""), x: cx(i) - cw * 0.32, width: cw * 0.64, y: top + strip - bh, height: bh, rx: 2 }));
        col.append(svg("text", { x: cx(i), y: top + strip - bh - 3, class: "bt-tick", "text-anchor": "middle" }, fmt.si(x.ess)));
        col.append(svg("text", { x: cx(i), y: H - 8, class: "bt-tick", "text-anchor": "middle" }, String(x.slot)));
        g.append(col);
      });
      g.append(svg("text", { x: L - 6, y: top + strip - 2, class: "bt-tick", "text-anchor": "end" }, "ess"));
      return [g, h("div", { class: "bt-legend small muted" }, h("span", {}, h("i", { class: "dot cdr" }), "cascade doubly robust"), h("span", {}, h("i", { class: "dot sn" }), "SNIPS (where it has the searches)"), h("span", {}, h("i", { class: "dia lg" }), "as logged"), h("span", {}, h("i", { class: "bar" }), "effective searches (paler: pages the log could not draw)"))];
    }

    async function tune() {
      const me = ++my;
      clear(tuneOut, card({ body: h("div", { class: "small muted" }, "Trying blends near this draft on half the searches…") }));
      try {
        const j = await api(`/v1/blends/${encodeURIComponent(name)}/tune`, { method: "POST", body: { export: exportBody(), start: { def: { ...clone(draft), name }, label: dirty() ? "this draft" : `${name}@${savedVersion}` }, rounds: 24, discount: st.discount } });
        if (me !== my) return;
        drawTune(j);
      } catch (e) { if (me === my) clear(tuneOut, card({ body: failure(e, "Tuning") })); }
    }

    function drawTune(j) {
      if (!j.tried?.length) { clear(tuneOut); howTo(tuneOut, "Nothing to tune on yet", "The log has no exploring pages of this blend."); return; }
      const start = j.tried[0].train.cdr.value;
      const sorted = j.tried.map((t, i) => ({ ...t, i })).sort((a, b) => b.train.cdr.value - a.train.cdr.value);
      const best = j.best, b = best?.estimate;
      const shown = sorted.slice(0, 12);
      const span = Math.max(1e-9, ...shown.map((t) => Math.abs(t.train.cdr.value - start)));
      const row = (t) => {
        const dv = t.train.cdr.value - start;
        const beyond = t.i !== 0 && t.train.coverage < 0.95;
        return h("div", { class: "bt-try" + (t.i === 0 ? " start" : "") + (best && t.sql === best.sql && t.i !== 0 ? " best" : "") + (beyond ? " beyond" : ""), title: beyond ? `${fmt.pct(1 - t.train.coverage, 0)} of its worth lies on pages the log could not draw: not chosen` : "" },
          h("span", { class: "small" }, t.i === 0 ? "the start" : t.change, beyond ? h("span", { class: "pill warn", style: { marginLeft: "6px" } }, "beyond the log") : null),
          h("span", { class: "bt-delta" }, h("i", { class: dv >= 0 ? "up" : "down", style: { width: `${(Math.abs(dv) / span) * 50}%`, [dv >= 0 ? "left" : "right"]: "50%" } })),
          h("span", { class: "mono small" }, `${dv >= 0 ? "+" : "−"}${num(Math.abs(dv))}`));
      };
      clear(tuneOut, card({
        title: "Tuned toward clicks",
        sub: `${j.tried.length - 1} blends near the start, each valued on one half of the ${fmt.n(j.gathered.requests)} searches`,
        body: [
          b ? h("div", { class: "bt-verdict" },
            h("div", { class: "row nw", style: { gap: "8px" } }, h("b", {}, "The best, on the other half"), h("span", { class: "pill " + ({ better: "ok", worse: "bad", "not proven": "warn" }[b.verdict] || "") }, b.verdict), h("span", { class: "grow" }), h("span", { class: "mono" }, `${num(b.cdr.value)} ± ${num(1.96 * b.cdr.se)}`)),
            h("div", { class: "small" }, `${b.lift.value >= 0 ? "+" : "−"}${num(Math.abs(b.lift.value))} ± ${num(1.96 * b.lift.se)} a search against the start there (${num(j.start?.cdr?.value)})`),
            h("div", { class: "small" }, best.def.note ? best.def.note.replace(/^tuned: /, "It changes ") : "It is the start: nothing near it did better."),
            b.verdict === "not proven" ? h("div", { class: "small muted" }, b.why) : null,
            h("div", { class: "row", style: { gap: "8px" } },
              h("button", { class: "btn sm primary", disabled: !best.def.note, onclick: () => { draft = { ...clone(best.def), name }; delete draft.note; d.close(); changed(); toast("The tuned blend is in the editor: look at its pages, then save it to serve it", "ok", 6000); } }, icon("edit"), "Open it in the editor"),
              h("button", { class: "btn sm ghost", onclick: () => copy(best.sql, "The SQL") }, icon("copy"), "SQL"))) : null,
          h("div", { class: "small muted" }, "Each blend tried, against the start, on the half that chose; one whose pages the log could mostly not have drawn is valued by the click model's guess, and not chosen"),
          h("div", { class: "bt-tries" }, shown.map(row)),
          sorted.length > shown.length ? h("div", { class: "small muted" }, `and ${sorted.length - shown.length} more, each worth less`) : null,
        ],
      }));
    }
  }

  async function drop() {
    const ok = await confirm({ title: `Drop ${name}?`, body: `Queries of blend('${name}') fail from now on. Its versions stay readable as ${name}@n, and saving it again makes the next version.`, ok: "Drop", danger: true, typed: name });
    if (!ok) return;
    try { await api(`/v1/blends/${encodeURIComponent(name)}`, { method: "DELETE" }); toast(`Dropped ${name}`, "ok"); location.hash = "#/blend"; } catch (e) { toast(e.message, "bad", 8000); }
  }

  return () => { alive = false; seq++; };
}

// ------------------------------------------------------------ small parts
const num = (x) => (x == null || !Number.isFinite(x) ? "–" : Math.abs(x) >= 100 ? x.toFixed(0) : Math.abs(x) >= 10 ? x.toFixed(1) : x.toFixed(3));
function niceTick(span) {
  const raw = span / 5, p = Math.pow(10, Math.floor(Math.log10(raw)));
  return [1, 2, 5, 10].map((m) => m * p).find((m) => m >= raw) || p * 10;
}
function fact(k, v, cls) { return h("span", { class: "bt-fact " + (cls || "") }, h("span", { class: "muted" }, k), " ", v); }

/** How to log a blend's exploring pages, with why there are none yet. */
function howTo(into, title, why) {
  const name = location.hash.match(/#\/blend\/([^?]+)/)?.[1] || "name";
  clear(into, card({ body: [
    empty("recsys", title, why),
    h("div", { class: "bt-howto" },
      h("div", { class: "small muted" }, "Serve it with \"log\" and \"explore\": each slot then draws among the rows that fit it by their utility (top 5 here, tau a share of their range), and the log keeps every row of every source and each slot's chance"),
      sqlBlock(`POST /v1/query\n{"sql": "SELECT slot, source, id, title FROM blend('${decodeURIComponent(name)}', q => :q)",\n "params": {"q": "rust"},\n "log": {"context": "u42", "explore": {"policy": "softmax", "tau": 0.3, "top": 5}}}`),
      h("div", { class: "small muted" }, "Keep what people did in a table of events (user, item, time), the user as the log's context. Then this panel values any blend of the same sources on those pages, and tunes one toward the clicks.")),
  ] }));
}

function askText(title, label) {
  return new Promise((resolve) => {
    let done = false;
    const inp = h("input", { class: "input mono", "aria-label": label, style: { width: "100%" } });
    const ok = h("button", { class: "btn primary" }, "Add");
    const m = modal({ title, body: h("label", { class: "bl-field" }, h("span", { class: "small dim" }, label), inp), foot: [h("button", { class: "btn", onclick: () => m.close() }, "Cancel"), ok], onClose: () => { if (!done) resolve(null); } });
    const go = () => { done = true; m.close(); resolve(inp.value.trim() || null); };
    ok.onclick = go;
    inp.onkeydown = (e) => { if (e.key === "Enter") go(); };
  });
}

// Focus back on the control of that label after a redraw, the caret at
// the end of a text.
function refocus(root, label) {
  if (!label) return;
  const el = root.querySelector(`[aria-label="${CSS.escape(label)}"]`);
  if (!el) return;
  el.focus();
  if (el.tagName === "INPUT" && el.type === "text" || el.tagName === "TEXTAREA") { const n = el.value.length; try { el.setSelectionRange(n, n); } catch { /* not a text */ } }
}

// A slider with its value; onInput as it moves, the draft runs when it stops.
function slider(min, max, step, value, onInput, { label, fmtv = (v) => v.toFixed(2) } = {}) {
  const out = h("span", { class: "small mono num bl-val" }, fmtv(value));
  const inp = h("input", { type: "range", min, max, step, value, class: "bl-range grow", "aria-label": label });
  inp.oninput = () => { out.textContent = fmtv(Number(inp.value)); onInput(Number(inp.value)); };
  inp.onchange = () => inp.closest(".bl-ed")?.dispatchEvent(new CustomEvent("bl-change", { bubbles: true }));
  return h("span", { class: "row nw grow", style: { gap: "8px" } }, inp, out);
}

// A whole number with − and + beside it.
function stepper(min, max, value, onChange, label) {
  const v = h("input", { class: "input sm num bl-step", type: "number", min, max, value, "aria-label": label });
  const set = (x) => { x = clamp(Math.round(Number(x) || min), min, max); v.value = x; onChange(x); };
  v.onchange = () => set(v.value);
  return h("span", { class: "bl-stepper" }, h("button", { type: "button", class: "btn ghost icon sm", "aria-label": `${label}: one fewer`, onclick: () => set(Number(v.value) - 1) }, icon("minus")), v, h("button", { type: "button", class: "btn ghost icon sm", "aria-label": `${label}: one more`, onclick: () => set(Number(v.value) + 1) }, icon("plus")));
}

// Two handles on one track: a quota's least and most share of its top n
// rows, in steps of one row; the mark is the share the slate has now.
function rangeQuota(q, n, now, onChange) {
  const track = h("div", { class: "bl-rt" });
  const band = h("i", { class: "band" });
  const mark = h("i", { class: "now", title: "the share the slate has now" });
  const hLo = h("button", { type: "button", class: "hd", "aria-label": "Least share", role: "slider", "aria-valuemin": 0, "aria-valuemax": 100 });
  const hHi = h("button", { type: "button", class: "hd", "aria-label": "Most share", role: "slider", "aria-valuemin": 0, "aria-valuemax": 100 });
  const step = 100 / Math.max(1, n);
  const snap = (v) => clamp(Math.round(Math.round(v / step) * step), 0, 100);
  const draw = () => {
    band.style.left = q.min + "%"; band.style.width = q.max - q.min + "%";
    hLo.style.left = q.min + "%"; hHi.style.left = q.max + "%";
    hLo.setAttribute("aria-valuenow", q.min); hHi.setAttribute("aria-valuenow", q.max);
    mark.style.display = now == null ? "none" : ""; if (now != null) mark.style.left = clamp(now * 100, 0, 100) + "%";
  };
  const drag = (which) => (e) => {
    e.preventDefault();
    const r = track.getBoundingClientRect();
    const mv = (ev) => { const v = snap(((ev.clientX - r.left) / r.width) * 100); if (which === "lo") q.min = Math.min(v, q.max); else q.max = Math.max(v, q.min); draw(); onChange(false); };
    const up = () => { removeEventListener("pointermove", mv); removeEventListener("pointerup", up); onChange(true); };
    addEventListener("pointermove", mv); addEventListener("pointerup", up); mv(e);
  };
  hLo.addEventListener("pointerdown", drag("lo")); hHi.addEventListener("pointerdown", drag("hi"));
  const key = (which) => (e) => {
    const d = e.key === "ArrowRight" || e.key === "ArrowUp" ? step : e.key === "ArrowLeft" || e.key === "ArrowDown" ? -step : 0;
    if (!d) return;
    e.preventDefault();
    if (which === "lo") q.min = clamp(snap(q.min + d), 0, q.max); else q.max = clamp(snap(q.max + d), q.min, 100);
    draw(); onChange(true);
  };
  hLo.addEventListener("keydown", key("lo")); hHi.addEventListener("keydown", key("hi"));
  track.addEventListener("pointerdown", (e) => { if (e.target !== track && e.target !== band) return; const r = track.getBoundingClientRect(); const v = ((e.clientX - r.left) / r.width) * 100; (Math.abs(v - q.min) <= Math.abs(v - q.max) ? drag("lo") : drag("hi"))(e); });
  track.append(h("i", { class: "trk" }), ...Array.from({ length: n + 1 }, (_, i) => h("i", { class: "tk", style: { left: i * step + "%" } })), band, mark, hLo, hHi);
  draw();
  return track;
}

// Query designer: a search designed stage by stage, as a template whose
// one input is the text a person types (qtemplate.js). One step for each
// stage of the funnel, in the order a query meets them: understanding,
// retrieval, ranking, blending; then save and use. Each step edits its
// stage and shows, beside it, what the stage does to a sample query: the
// words it understood or corrected, the rows each input of the retrieval
// found and the terms its words became, the rows the ranking moved, the
// page the blend made. The funnel under the stepper says how many rows
// each stage kept (POST /v1/funnel, its rows funnel drawn as the Query
// funnel draws it). Nothing is stored until Save, which runs the CREATE
// statements of a rank profile, a blend, and the stored functions apps
// and agents call (fn_<name> over MCP).

import { h, s as svg, icon, clear, api, store, toast, copy, card, empty, seg, toggle, sqlBlock, debounce, failure, gate, can, fmt, link, menu } from "../core.js";
import { isProduction } from "../data.js";
import { tables, table as tableOf, operators } from "../catalog.js";
import * as T from "../qtemplate.js";
import { rowsFunnel, termsOf } from "./funnel.js";
import { useFromCode } from "./saved.js";

export default { id: "designer", title: "Query designer", icon: "stages", group: "Build", range: false, keywords: "query designer template search pipeline stage by stage steps funnel retrieval understanding ranking blending words meaning typos synonyms expansion feedback rm3 boost rerank cross encoder diversify dedupe quota rank profile blend function saved search reusable", render, onParams };

let reopen = null;
function onParams(r) { reopen?.(r.params); }

/** The steps, in order: [id, title, what the stage does]. */
export const STEPS = [
  ["understanding", "Understanding", "What the words say beyond their words: a value of the table a word names (a brand, a colour, “under 200”) becomes a boost or a filter."],
  ["retrieval", "Retrieval", "The candidates: rows found by their words (with typos, synonyms and terms from the first rows) and by meaning, kept by fixed filters, fused, cut to the top N."],
  ["ranking", "Ranking", "The candidates ranked again: the retrieval's score boosted, then its top rows scored by heads, each a prediction (words, an embedding, a number, a learned model, a cross-encoder) with a weight, fused into one score."],
  ["blending", "Blending", "The page made of several queries: this template's rows with other templates', a table's or any query's, spread across values, capped per value, without near duplicates, a share for a kind."],
  ["save", "Save and use", "Kept as engine objects: a rank profile, a blend, and the function apps and agents call by name."],
];
const RERANKERS = ["ms-marco-MiniLM-L-6-v2", "ms-marco-MiniLM-L-6-v2-int8", "bge-reranker-base", "cohere/rerank-v3.5", "voyage/rerank-2.5", "jina/jina-reranker-v2-base-multilingual", "anthropic/claude-haiku-5-5"];
/** The most rows a cross-encoder scores unless the server says otherwise (--max-rerank). */
const MAX_RERANK = 20;
const clip = (s, n = 90) => { s = s == null ? "" : typeof s === "object" ? JSON.stringify(s) : String(s); return s.length > n ? s.slice(0, n - 1) + "…" : s; };
const keyText = (k) => (typeof k === "string" ? k : JSON.stringify(k));

function render(page, r) {
  let t = null, d = null, step = "understanding", saved = [], fnNames = new Set(), live = !isProduction(), alive = true;
  let sample = "";
  let pv = {};          // the preview's answers, by part
  let seq = 0;
  // How a step's change is drawn: the flow between the lists, colored by
  // how far each row moved, or the plain lists side by side.
  let view = store.get("designer.view", "flow") === "lists" ? "lists" : "flow";
  const head = h("div", { class: "page-head" },
    h("div", {}, h("h1", {}, "Query designer"), h("p", {}, "A search designed stage by stage. Its input is the text a person types; each stage shows what it does to a sample query, and the funnel what each one keeps.")));
  const tools = h("div", { class: "tools" });
  head.append(tools);
  const top = h("div", { class: "dz-top card" });
  const stepper = h("ol", { class: "dz-steps", "aria-label": "Stages" });
  const funnelBox = h("div", { class: "dz-funnel" });
  const editor = h("div", { class: "dz-edit" });
  const preview = h("div", { class: "dz-preview" });
  page.append(h("div", { class: "dz" }, head, top, stepper, h("div", { class: "dz-main" }, editor, h("div", { class: "dz-side" }, preview, funnelBox))));

  const keep = debounce(() => { if (d) store.set("designer.draft", { d, step, sample }); }, 300);
  const previewSoon = debounce(() => { if (live) runPreview(); }, 400);
  function changed({ redraw = true } = {}) { keep(); drawStepper(); if (redraw) drawEditor(); if (live) { preview.classList.add("busy"); previewSoon(); } else drawPreview(); }

  // ------------------------------------------------------------ loading
  async function loadSaved() {
    try {
      const j = await api("/v1/functions");
      saved = (j.functions || []).filter((f) => f.current != null).map((f) => {
        const v = f.versions.find((x) => x.version === f.current) || {};
        const design = T.ofBody(v.body);
        return design && f.name.endsWith("_template") ? { name: design.name || f.name.replace(/_template$/, ""), version: f.current, design, created: v.created, by: v.created_by } : null;
      }).filter(Boolean);
      fnNames = new Set((j.functions || []).filter((f) => f.current != null).map((f) => f.name));
    } catch { saved = []; }
    drawTools();
  }
  function drawTools() {
    const open = h("button", { class: "btn", title: "Open a saved template", onclick: () => {
      menu(open, saved.length ? saved.map((s) => ({ label: `${s.name} · ${s.design.table} · version ${s.version}`, icon: "stages", onclick: () => openTemplate(s.name) })) : [{ label: "No saved templates yet", icon: "info", onclick: () => {} }]);
    } }, icon("down"), saved.length ? `Templates · ${saved.length}` : "Templates");
    clear(tools, open, h("button", { class: "btn", onclick: () => newTemplate(t?.table) }, icon("plus"), "New template"));
  }
  function openTemplate(name) {
    const s = saved.find((x) => x.name === name);
    if (!s) { toast(`No saved template ${name}`, "bad"); return; }
    const tt = tableOf(s.design.table);
    if (!tt) { toast(`${name} reads ${s.design.table}, which is not here`, "bad"); return; }
    t = tt; d = T.normalize(s.design, t); d.savedVersion = s.version; d.hadBlend = !!s.design.blend?.on;
    sample = d.samples[0] || sample;
    pv = {}; step = "understanding";
    drawAll(); if (live) runPreview();
  }
  function newTemplate(name) {
    const tt = tableOf(name) || usable()[0];
    if (!tt) return;
    t = tt; d = T.blank(t, "");
    pv = {};
    drawAll(); if (live && sample) runPreview();
  }
  const usable = () => (cachedTables || []).filter((x) => !x.table.startsWith("_") && (T.textFields(x).length || T.vectorFields(x).some((f) => f.embed)));
  let cachedTables = null;

  Promise.all([tables(), loadSaved()]).then(([ts]) => {
    if (!alive) return;
    cachedTables = ts;
    if (!usable().length) { clear(editor, card({ body: empty("builder", "No table to search yet", "A template searches a table with a TEXT field, or a vector field a model fills. Add one, then come back.", h("a", { class: "btn primary", href: "#/start" }, icon("rocket"), "Start here")) })); return; }
    const want = r.params.get("template"), wantTable = r.params.get("table");
    const kept = store.get("designer.draft", null);
    if (want && saved.some((s) => s.name === want)) return openTemplate(want);
    if (kept?.d && tableOf(kept.d.table) && !wantTable) { t = tableOf(kept.d.table); d = T.normalize(kept.d, t); d.savedVersion = kept.d.savedVersion; step = kept.step || step; sample = kept.sample || ""; }
    else newTemplate(wantTable);
    drawAll();
    if (!sample) guessSample(); else if (live) runPreview();
  }).catch((e) => clear(editor, failure(e, "The tables")));
  reopen = (params) => { const n = params.get("template"); if (n && d?.name !== n) openTemplate(n); };

  /** A sample to start from: the template's own, else a few words of a row of the table. */
  async function guessSample() {
    if (d?.samples?.[0]) sample = d.samples[0];
    else {
      const col = d?.output.columns.find((c) => T.fieldsOf(t).find((f) => f.name === c && f.type === "text")) || d?.words.fields[0]?.field;
      if (col) {
        try {
          const j = await api("/v1/query", { body: { sql: `SELECT ${col} FROM ${t.table} WHERE ${col} IS NOT NULL LIMIT 1` } });
          const words = String(j.rows?.[0]?.[0] || "").toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) || [];
          sample = words.slice(0, 3).join(" ");
        } catch { /* left empty: the page asks for one */ }
      }
    }
    if (!alive) return;
    drawTop(); if (live && sample) runPreview(); else drawPreview();
  }

  function drawAll() { drawTop(); drawStepper(); drawEditor(); drawPreview(); }

  // ------------------------------------------------------------ the top: name, table, sample
  function drawTop() {
    if (!d) return;
    const nameIn = h("input", { class: "input mono", value: d.name, placeholder: "product_search", spellcheck: "false", "aria-label": "Template name", oninput: () => { d.name = T.slug(nameIn.value); keep(); drawStepper(); if (step === "save") drawEditor(); } });
    const tsel = h("select", { class: "select", "aria-label": "Table", onchange: () => {
      const tt = tableOf(tsel.value); if (!tt) return;
      const was = d; t = tt; d = T.blank(t, was.name); d.samples = was.samples; d.about = was.about; pv = {}; changed();
      drawTop();
    } }, usable().map((x) => h("option", { value: x.table, selected: x.table === t.table }, x.table)));
    const sampleIn = h("input", { class: "input dz-sample", value: sample, placeholder: "A query as a person types it: waterprof hiking boots under 200", "aria-label": "Sample query", onkeydown: (e) => { if (e.key === "Enter") { sample = sampleIn.value.trim(); keep(); runPreview(); } }, oninput: () => { sample = sampleIn.value; keep(); if (live) { preview.classList.add("busy"); previewSoon(); } } });
    const chips = h("div", { class: "row dz-chips" },
      d.samples.map((s, i) => h("span", { class: "chip" + (s === sample ? " on" : ""), "aria-pressed": String(s === sample), onclick: () => { sample = s; sampleIn.value = s; keep(); runPreview(); } }, s, h("span", { class: "x", title: "Remove this sample", onclick: (e) => { e.stopPropagation(); d.samples.splice(i, 1); keep(); drawTop(); } }, "×"))),
      h("button", { class: "btn xs ghost", title: "Keep this query with the template, to try each stage on it again", onclick: () => { const s = sampleIn.value.trim(); if (s && !d.samples.includes(s)) { d.samples.push(s); keep(); drawTop(); } } }, icon("plus"), "Keep as a sample"));
    const liveIn = toggle(live, (v) => { live = v; if (live) runPreview(); }, "preview as I change");
    clear(top,
      h("div", { class: "dz-top-row" },
        h("label", { class: "f" }, "Template", nameIn),
        h("label", { class: "f" }, "Searches", tsel),
        h("label", { class: "f grow" }, "Sample query (the template's input, :q)", h("div", { class: "row nw" }, sampleIn, h("button", { class: "btn primary", onclick: () => { sample = sampleIn.value.trim(); keep(); runPreview(); } }, icon("play"), "Preview")))),
      h("div", { class: "dz-top-row small" }, chips, h("span", { class: "grow" }),
        h("span", { class: "dim", title: isProduction() ? "This server counts as production: a change previews when you press Preview" : "" }, liveIn),
        h("span", { class: "dim" }, d.savedVersion ? `Saved as version ${d.savedVersion}; changes are a draft until you save.` : "A draft: the preview runs queries, and nothing is stored until you save.")));
  }

  // ------------------------------------------------------------ the stepper: one column a stage
  function drawStepper() {
    if (!d) return;
    let sum = {};
    try { sum = T.summary(d, t); } catch { /* shown in the editor */ }
    const counts = stageCounts();
    clear(stepper, STEPS.map(([id, title], i) => {
      const at = STEPS.findIndex((x) => x[0] === step);
      return h("li", { class: "dz-step" + (id === step ? " on" : "") + (i < at ? " done" : ""), onclick: () => { step = id; keep(); drawStepper(); drawEditor(); drawPreview(); if (live && needs(id)) runPreview(); } },
        h("button", { class: "dz-step-b", "aria-current": id === step ? "step" : null },
          h("span", { class: "start-n" }, String(i + 1)),
          h("span", { class: "dz-step-t" }, h("b", {}, title), h("span", { class: "small dim dz-step-s" }, id === "save" ? (d.name ? `as ${d.name}()` : "name it first") : sum[id] || "")),
          counts[id] != null ? h("span", { class: "dz-step-n num", title: counts[id].title }, counts[id].text) : null));
    }));
  }
  /** What each stage kept, from the latest funnel: {stage: {text, title}}. */
  function stageCounts() {
    const rows = pv.funnel?.rows, out = {};
    if (!rows || rows.kind === "other") return out;
    const und = pv.funnel?.text?.understood?.length;
    if (d.understand.values) out.understanding = { text: und ? `${und} read` : "–", title: "lines the understanding wrote" };
    if (rows.candidates != null || rows.matched != null) out.retrieval = { text: fmt.si(rows.candidates ?? rows.matched), title: `${fmt.n(rows.matched ?? 0)} matched, ${fmt.n(rows.candidates ?? rows.selected ?? 0)} candidates` };
    out.ranking = { text: fmt.n(rows.shown), title: `${fmt.n(rows.ranked)} ranked, ${fmt.n(rows.shown)} on the page` };
    if (d.blend.on && pv.blend?.rows) out.blending = { text: fmt.n(pv.blend.rows.length), title: "slots placed" };
    return out;
  }

  // ------------------------------------------------------------ the editor of each stage
  const F = (n) => T.fieldsOf(t).find((f) => f.name === n);
  const fieldSelect = (list, value, onChange, { none = null, label = "" } = {}) => {
    const s = h("select", { class: "select", "aria-label": label, onchange: () => onChange(s.value) }, none != null ? h("option", { value: "", selected: !value }, none) : null, list.map((f) => h("option", { value: f.name, selected: f.name === value }, f.name)));
    return s;
  };
  const numIn = (value, onChange, attrs = {}) => { const i = h("input", { class: "input sm num", type: "number", value: value ?? "", style: { width: "84px" }, ...attrs, oninput: () => onChange(i.value === "" ? null : Number(i.value)) }); return i; };
  const section = (title, sub, ...body) => h("div", { class: "dz-sec" }, h("div", { class: "dz-sec-h" }, h("b", {}, title), sub ? h("span", { class: "small dim" }, sub) : null), ...body);
  const opt = (label, what, control) => h("div", { class: "dz-opt" }, h("div", { class: "dz-opt-t" }, h("span", {}, label), what ? h("span", { class: "small dim" }, what) : null), h("div", { class: "dz-opt-c" }, control));

  function drawEditor() {
    if (!d) return;
    const [id, title, what] = STEPS.find((x) => x[0] === step);
    const i = STEPS.findIndex((x) => x[0] === step);
    let body;
    try { body = { understanding: editUnderstanding, retrieval: editRetrieval, ranking: editRanking, blending: editBlending, save: editSave }[id](); }
    catch (e) { body = h("div", { class: "error" }, e.message); }
    const nav = h("div", { class: "dz-nav" },
      i > 0 ? h("button", { class: "btn", onclick: () => go(STEPS[i - 1][0]) }, icon("left"), STEPS[i - 1][1]) : h("span"),
      h("span", { class: "grow" }),
      i < STEPS.length - 1 ? h("button", { class: "btn" + (STEPS[i + 1][0] === "save" ? "" : ""), onclick: () => go(STEPS[i + 1][0]) }, STEPS[i + 1][1], icon("right")) : null);
    clear(editor, card({ title: h("span", {}, h("span", { class: "dz-n" }, String(i + 1)), title), body: [h("p", { class: "dz-what" }, what), body, nav] }));
  }
  function go(id) { step = id; keep(); drawStepper(); drawEditor(); drawPreview(); if (live && needs(id)) runPreview(); window.scrollTo?.({ top: 0, behavior: "smooth" }); }

  function editUnderstanding() {
    const u = d.understand;
    const cands = [...T.keywordFields(t), ...T.numberFields(t)];
    const primary = d.words.fields[0]?.field;
    const chips = h("div", { class: "row dz-chips" }, cands.map((f) => {
      const on = u.fields.includes(f.name);
      return h("button", { class: "chip", "aria-pressed": String(on), title: f.type === "keyword" ? "a word may name one of its values" : "a range of it may be named: under 200, over 4", onclick: () => { u.fields = on ? u.fields.filter((x) => x !== f.name) : [...u.fields, f.name]; changed(); } }, f.name, h("span", { class: "dim" }, f.type === "keyword" ? " values" : " ranges"));
    }));
    return h("div", { class: "stack" },
      !d.words.on || !primary ? h("div", { class: "notice" }, "Understanding reads the words of the retrieval's first words field: turn words on in Retrieval to use it.") : null,
      section("Values the words name", `read against ${primary || "the first words field"}: the rest of the words are searched`,
        opt("Read values", "a word that names a value of a field, or a range of a number", toggle(u.values, (v) => { u.values = v; if (v && !u.fields.length) u.fields = cands.filter((f) => f.type === "keyword" && !f.multi).slice(0, 3).map((f) => f.name); changed(); })),
        u.values ? opt("Fields", "the fields whose values or ranges a query may name", cands.length ? chips : h("span", { class: "small dim" }, `${t.table} has no KEYWORD or number field`)) : null,
        u.values ? opt("What a named value does", "boost: rows with it rank higher; filter: only rows with it (or without the field) stay",
          seg([["boost", "Boost"], ["filter", "Filter"]], u.mode, (v) => { u.mode = v; changed({ redraw: false }); })) : null,
        u.values && u.mode === "boost" ? opt("Boost", "each condition a row meets multiplies its words' score by 1 + this", numIn(u.weight, (v) => { u.weight = v ?? 0.25; changed({ redraw: false }); }, { step: 0.05, min: 0 })) : null),
      section("Spelling", "Typos are corrected inside the query by Retrieval's typo tolerance. The preview shows what “did you mean” would say.",
        opt("Typo tolerance", "set in Retrieval", h("span", { class: "badge" }, d.words.typos === "off" ? "off" : d.words.typos === "auto" ? "auto" : `${d.words.typos} edits`))));
  }

  function editRetrieval() {
    const w = d.words, m = d.meaning;
    const texts = T.textFields(t), vecs = T.vectorFields(t);
    const fieldRows = h("div", { class: "stack", style: { gap: "6px" } }, w.fields.map((x, i) => h("div", { class: "row nw dz-frow" },
      fieldSelect(texts, x.field, (v) => { x.field = v; changed(); }, { label: "Words field" }),
      h("span", { class: "small dim" }, "weight"), numIn(x.weight, (v) => { x.weight = v ?? 1; changed({ redraw: false }); }, { step: 0.1, min: 0 }),
      i === 0 ? h("span", { class: "badge", title: "Understanding and the all-words filter read this field" }, "first") : null,
      h("button", { class: "btn xs ghost", title: "Remove", onclick: () => { w.fields.splice(i, 1); changed(); } }, icon("x")))),
      texts.length > w.fields.length ? h("button", { class: "btn xs", onclick: () => { const f = texts.find((x) => !w.fields.some((y) => y.field === x.name)); if (f) { w.fields.push({ field: f.name, weight: 0.5 }); changed(); } } }, icon("plus"), "Another field") : null);
    const matchOpts = [["any", "Any word", "a row needs one of the words; more words rank higher"], ["all", "All words", "a row needs every word (MATCH)"]];
    const single = (w.fields.length === 1 && !(m.on && m.field)) && !(Number(w.feedback) > 0);
    if (single) matchOpts.push(["relax", "All, relaxing", "all the words while enough rows have them, else one fewer at a time"]);
    if (w.match === "relax" && !single) w.match = "any";
    const filterRows = h("div", { class: "stack", style: { gap: "6px" } }, d.filters.map((c, i) => {
      const f = F(c.field);
      const ops = f ? operators(f).filter(([o]) => o !== "matches") : [];
      const opSel = h("select", { class: "select sm", onchange: () => { c.op = opSel.value; changed(); } }, ops.map(([o, l]) => h("option", { value: o, selected: o === c.op }, l)));
      const needsValue = !["null", "notnull", "true", "false"].includes(c.op);
      return h("div", { class: "row nw dz-frow" },
        fieldSelect(T.fieldsOf(t).filter((x) => operators(x).length && x.type !== "text" && x.type !== "vector"), c.field, (v) => { c.field = v; c.op = operators(F(v))[0]?.[0] || "="; changed(); }, { label: "Filter field" }),
        opSel,
        needsValue ? h("input", { class: "input sm", value: c.value ?? "", placeholder: c.op === "in" ? "a, b, c" : "value", oninput: (e) => { c.value = e.target.value; changed({ redraw: false }); } }) : null,
        c.op === "between" ? h("input", { class: "input sm", value: c.value2 ?? "", placeholder: "and", oninput: (e) => { c.value2 = e.target.value; changed({ redraw: false }); } }) : null,
        h("button", { class: "btn xs ghost", title: "Remove", onclick: () => { d.filters.splice(i, 1); changed(); } }, icon("x")));
    }), h("button", { class: "btn xs", onclick: () => { const f = T.fieldsOf(t).find((x) => x.type !== "text" && x.type !== "vector" && operators(x).length); if (f) { d.filters.push({ field: f.name, op: operators(f)[0][0], value: "" }); changed(); } } }, icon("plus"), "A filter"));
    const both = w.on && w.fields.length && m.on && m.field;
    return h("div", { class: "stack" },
      section("Words", "BM25 over the fields' words",
        opt("Search the words", null, toggle(w.on, (v) => { w.on = v; if (v && !w.fields.length && texts[0]) w.fields.push({ field: texts[0].name, weight: 1 }); changed(); })),
        w.on ? opt("Fields", "each one an input of the retrieval, fused with the others", texts.length ? fieldRows : h("span", { class: "small dim" }, `${t.table} has no indexed TEXT field`)) : null,
        w.on ? opt("Which rows", null, seg(matchOpts.map(([v, l, tt]) => [v, l, tt]), w.match, (v) => { w.match = v; changed({ redraw: false }); })) : null,
        w.on ? opt("Typed as a person types", "quotes, signs and brackets in the text are words, not operators", toggle(w.plain, (v) => { w.plain = v; changed({ redraw: false }); })) : null),
      w.on ? section("Expansions", "the words become more terms: the preview shows each term and the rows that hold it",
        opt("Typos", "also the words an edit or two away; auto corrects only the words that look misspelt", seg([["off", "Off"], ["auto", "Auto"], ["1", "1 edit"], ["2", "2 edits"]], String(w.typos), (v) => { w.typos = v; changed(); })),
        opt("Synonyms", "the server's list and the table's own (Understanding → Configure makes one)", toggle(w.synonyms, (v) => { w.synonyms = v; changed({ redraw: false }); })),
        opt("Feedback from the first rows", "RM3: words the top N rows hold most join the query (0 is off); more recall, sometimes off topic", numIn(w.feedback, (v) => { w.feedback = Math.max(0, Math.min(100, v || 0)); changed({ redraw: false }); }, { min: 0, max: 100, step: 1 }))) : null,
      section("Meaning", vecs.length ? "nearest vectors to the text, which the server embeds with the field's model" : `${t.table} has no vector field: add one under Understanding → Enrich rows (Vector for meaning)`,
        vecs.length ? opt("Search by meaning", null, toggle(m.on, (v) => { m.on = v; if (v && !m.field) m.field = (vecs.find((f) => f.embed) || vecs[0]).name; changed(); })) : null,
        m.on && vecs.length ? opt("Vector field", null, h("div", { class: "row nw" }, fieldSelect(vecs, m.field, (v) => { m.field = v; changed(); }, { label: "Vector field" }),
          F(m.field)?.embed ? h("span", { class: "small dim" }, `by ${F(m.field).embed.model}`) : h("span", { class: "small", style: { color: "var(--serious)" } }, "no model embeds it: the text cannot become a vector"))) : null,
        m.on && both ? opt("Weight", "against the words fields' weights", numIn(m.weight, (v) => { m.weight = v ?? 1; changed({ redraw: false }); }, { step: 0.1, min: 0 })) : null),
      inputsCount() > 1 ? section("Fusion", "how the inputs' rankings become one",
        opt("Fuse by", null, seg([["rrf", "Rank (RRF)", "each input's rank counts, not its score: robust to scores on other scales"], ["weighted", "Weighted scores", "each input's score scaled to its best (tmm), then weighted"]], d.fusion, (v) => { d.fusion = v; changed({ redraw: false }); }))) : null,
      section("Fixed filters", "conditions every query of the template keeps: in stock, a language, a status", filterRows),
      inputsCount() > 1 ? section("Candidates", "how many rows each input gives the fusion",
        opt("Top N of each input", "the fused ranking reads these; more finds more, and costs more", numIn(d.candidates, (v) => { d.candidates = Math.max(1, Math.min(10000, v || 100)); changed({ redraw: false }); }, { min: 1, max: 10000, step: 10 }))) :
        h("div", { class: "small dim" }, "One input: every row it matches is ranked, boosts included, so no candidate cut is needed."));
  }
  const inputsCount = () => { try { return T.inputs(d, t).length; } catch { return 0; } };

  function editRanking() {
    const rk = d.rank;
    const nums = T.numberFields(t), times = T.timeFields(t);
    const kinds = [["number", "More of a number", nums], ["recency", "Newer first", [...times, ...nums.filter((f) => /year|date|time|ts|created|updated|published/i.test(f.name))]], ["closeness", "Close to a value", nums], ["condition", "A condition holds", T.fieldsOf(t).filter((f) => f.type !== "vector" && f.type !== "text" && operators(f).length)]];
    const rows = h("div", { class: "stack", style: { gap: "8px" } }, rk.boosts.map((b, i) => {
      const kind = kinds.find((k) => k[0] === b.kind) || kinds[0];
      const f = F(b.field);
      const parts = [
        h("select", { class: "select sm", onchange: (e) => { b.kind = e.target.value; const k = kinds.find((x) => x[0] === b.kind); if (!k[2].some((x) => x.name === b.field)) b.field = k[2][0]?.name; changed(); } }, kinds.filter((k) => k[2].length).map(([v, l]) => h("option", { value: v, selected: v === b.kind }, l))),
        fieldSelect(kind[2], b.field, (v) => { b.field = v; changed(); }, { label: "Boost field" }),
      ];
      if (b.kind === "number") parts.push(h("label", { class: "row nw small dim" }, toggle(!!b.log, (v) => { b.log = v; changed({ redraw: false }); }), "log", h("span", { title: "ln(1 + value): for counts, where the first ones matter most" }, "")));
      if (b.kind === "recency") parts.push(h("span", { class: "small dim" }, "half-life, days"), numIn(b.days ?? 30, (v) => { b.days = v || 30; changed({ redraw: false }); }, { min: 1 }));
      if (b.kind === "closeness") parts.push(h("span", { class: "small dim" }, "near"), numIn(b.origin, (v) => { b.origin = v; changed({ redraw: false }); }), h("span", { class: "small dim" }, "within"), numIn(b.scale ?? 1, (v) => { b.scale = v || 1; changed({ redraw: false }); }));
      if (b.kind === "condition" && f) {
        const ops = operators(f).filter(([o]) => !["in", "between", "matches"].includes(o));
        parts.push(h("select", { class: "select sm", onchange: (e) => { b.op = e.target.value; changed(); } }, ops.map(([o, l]) => h("option", { value: o, selected: o === (b.op || "=") }, l))));
        if (!["null", "notnull", "true", "false"].includes(b.op || "=")) parts.push(h("input", { class: "input sm", value: b.value ?? "", placeholder: "value", oninput: (e) => { b.value = e.target.value; changed({ redraw: false }); } }));
      }
      parts.push(h("span", { class: "small dim", title: "the score is multiplied by 1 + weight × the boost: a parameter of the rank profile, which the relevance autopilot can tune" }, "weight"), numIn(b.w ?? 0.1, (v) => { b.w = v ?? 0; changed({ redraw: false }); }, { step: 0.05, min: 0 }));
      parts.push(h("button", { class: "btn xs ghost", title: "Remove", onclick: () => { rk.boosts.splice(i, 1); changed(); } }, icon("x")));
      return h("div", { class: "row dz-frow", style: { flexWrap: "wrap" } }, parts);
    }), h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } }, kinds.filter((k) => k[2].length).map(([v, l, fs]) => h("button", { class: "btn xs", onclick: () => { rk.boosts.push({ kind: v, field: fs[0].name, w: v === "condition" ? 0.2 : 0.1, ...(v === "number" && /count|sold|reviews|clicks|points|views/i.test(fs[0].name) ? { log: true } : {}) }); changed(); } }, icon("plus"), l))));
    const hd = rk.heads;
    const headRows = h("div", { class: "stack", style: { gap: "8px" } }, hd.list.map((x, i) => headRow(x, () => { hd.list.splice(i, 1); changed(); })),
      h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } }, T.HEAD_KINDS.filter(([k]) => headFields(k).length || k === "model" || k === "cross").map(([k, l, tt]) => h("button", { class: "btn xs", title: tt, onclick: () => { hd.list.push(newHead(k)); if (k === "cross" && hd.depth > MAX_RERANK) hd.depth = MAX_RERANK; changed(); } }, icon("plus"), l))));
    const hasCross = hd.list.some((x) => x.kind === "cross");
    return h("div", { class: "stack" },
      section("Boosts", "each one multiplies the retrieval's score by 1 + weight × its value, over every candidate", rows),
      section("Heads", "rank the top rows again by predictions of your choice, each with a weight, fused into one score: a second phase over the rows the boosts ranked first",
        opt("Rank again by heads", "off: the boosted retrieval is the ranking", toggle(hd.on, (v) => { hd.on = v; if (v && !hd.list.length) hd.list = starterHeads(); changed(); })),
        hd.on ? opt("Rows the heads read", hasCross ? `the first phase's top rows; a cross-encoder reads at most ${MAX_RERANK} unless the server allows more (--max-rerank)` : "the first phase's top rows: more finds more, and costs more", numIn(hd.depth, (v) => { hd.depth = Math.max(1, Math.min(10000, v || 100)); changed({ redraw: false }); }, { min: 1, max: 10000 })) : null,
        hd.on ? h("div", { class: "stack", style: { gap: "6px" } }, h("div", { class: "small dim" }, "Each head is a prediction of how good a row is, with its weight; a weight below 0 ranks it lower."), headRows) : null,
        hd.on && hd.list.length > 1 ? opt("Fuse by", "how the heads' scores become one",
          h("div", { class: "row nw" }, seg([["weighted", "Weighted scores", "each head scaled over the rows it reads, times its weight, summed"], ["rrf", "Rank (RRF)", "each head's rank counts, not its score, weighted"]], hd.fuse, (v) => { hd.fuse = v; changed(); }),
            hd.fuse !== "rrf" ? seg([["minmax", "minmax", "0 for the least of the rows read, 1 for the best"], ["zscore", "zscore", "standard deviations from the rows' mean"]], hd.norm, (v) => { hd.norm = v; changed({ redraw: false }); }) : null)) : null,
        hd.on && hasCross && hd.depth > MAX_RERANK ? h("div", { class: "notice" }, icon("warn"), ` A cross-encoder reads ${hd.depth} rows here: a server allows ${MAX_RERANK} unless started with a higher --max-rerank.`) : null),
      section("The page", null,
        opt("Rows on a page", "the template's k, which a call may change", numIn(d.output.k, (v) => { d.output.k = Math.max(1, Math.min(1000, v || 10)); changed({ redraw: false }); }, { min: 1, max: 1000 })),
        opt("Columns", "what each row answers, besides its score", h("div", { class: "row dz-chips" }, T.showable(t).map((f) => { const on = d.output.columns.includes(f.name); return h("button", { class: "chip", "aria-pressed": String(on), onclick: () => { d.output.columns = on ? d.output.columns.filter((x) => x !== f.name) : [...d.output.columns, f.name]; changed(); } }, f.name); })))));
  }

  /** The fields a head of kind `k` reads. */
  function headFields(k) {
    switch (k) {
      case "words": return T.textFields(t);
      case "meaning": return T.vectorFields(t);
      case "number": return T.numberFields(t);
      case "recency": return T.timeFields(t);
      case "cross": return T.fieldsOf(t).filter((f) => (f.type === "text" || f.type === "keyword") && f.stored !== false && !(f.from && f.from.length));
      default: return [];
    }
  }
  function newHead(k) {
    const f = headFields(k)[0]?.name;
    if (k === "model") return { kind: k, model: "", prob: false, features: starterHeads().map(({ kind, field }) => ({ kind, field })), w: 1 };
    if (k === "cross") return { kind: k, model: RERANKERS[0], fields: [], w: 1 };
    return { kind: k, field: f, w: k === "words" || k === "meaning" ? 1 : 0.2, ...(k === "number" && /count|sold|reviews|clicks|points|views/i.test(f || "") ? { log: true } : {}) };
  }
  /** Heads to start from: the retrieval's own signals, again. */
  function starterHeads() {
    const out = d.words.on ? d.words.fields.slice(0, 2).map((x) => ({ kind: "words", field: x.field, w: x.weight ?? 1 })) : [];
    if (d.meaning.on && d.meaning.field) out.push({ kind: "meaning", field: d.meaning.field, w: 0.5 });
    if (!out.length && T.vectorFields(t)[0]) out.push({ kind: "meaning", field: T.vectorFields(t)[0].name, w: 1 });
    return out;
  }
  function featureChips(x) {
    const opts = ["words", "meaning", "number", "recency"].flatMap((k) => headFields(k).flatMap((f) => k === "number" ? [{ kind: k, field: f.name }, { kind: k, field: f.name, log: true }] : [{ kind: k, field: f.name }]));
    const key = (y) => `${y.kind}:${y.field}:${y.log ? 1 : 0}`;
    const on = new Set((x.features || []).map(key));
    return h("div", { class: "stack", style: { gap: "4px" } },
      h("div", { class: "row dz-chips" }, (x.features || []).map((y, i) => h("span", { class: "chip on" }, h("span", { class: "dim" }, `${i + 1}. `), T.headLabel(y), h("span", { class: "x", title: "Remove", onclick: () => { x.features.splice(i, 1); changed(); } }, "×")))),
      h("div", { class: "row dz-chips" }, opts.filter((y) => !on.has(key(y))).map((y) => h("button", { class: "chip", title: "Add as the next feature", onclick: () => { x.features = [...(x.features || []), y]; changed(); } }, "+ ", T.headLabel(y)))));
  }
  function headRow(x, remove) {
    const kinds = T.HEAD_KINDS.filter(([k]) => headFields(k).length || k === "model" || k === "cross");
    const parts = [h("select", { class: "select sm", "aria-label": "Head kind", onchange: (e) => { Object.assign(x, newHead(e.target.value), { w: x.w }); changed(); } }, kinds.map(([k, l]) => h("option", { value: k, selected: k === x.kind }, l)))];
    let more = null;
    if (["words", "meaning", "number", "recency"].includes(x.kind)) {
      parts.push(fieldSelect(headFields(x.kind), x.field, (v) => { x.field = v; changed(); }, { label: "Head field" }));
      if (x.kind === "meaning") { const f = F(x.field); parts.push(f?.embed ? h("span", { class: "small dim" }, `by ${f.embed.model}`) : h("span", { class: "small", style: { color: "var(--serious)" } }, "no model embeds it")); }
      if (x.kind === "number") parts.push(h("label", { class: "row nw small dim", title: "ln(1 + value): for counts, where the first ones matter most" }, toggle(!!x.log, (v) => { x.log = v; changed({ redraw: false }); }), "log"));
      if (x.kind === "recency") parts.push(h("span", { class: "small dim" }, "half-life, days"), numIn(x.days ?? 30, (v) => { x.days = v || 30; changed({ redraw: false }); }, { min: 1 }));
    } else if (x.kind === "model") {
      const m = h("input", { class: "input sm mono", value: x.model || "", placeholder: "clicks_ltr", title: "<models>/<name>/gbdt.json on the server: LightGBM's dump_model() or XGBoost's save_model()", style: { width: "160px" }, onchange: () => { x.model = m.value.trim(); changed(); } });
      parts.push(m, h("label", { class: "row nw small dim", title: "sigmoid of the score: a binary model's probability" }, toggle(!!x.prob, (v) => { x.prob = v; changed({ redraw: false }); }), "probability"));
      more = h("div", { class: "dz-sub" }, h("span", { class: "small dim" }, "Its features, in the order it was trained on:"), featureChips(x));
    } else if (x.kind === "cross") {
      const m = h("input", { class: "input sm mono", list: "dz-rerankers", value: x.model || "", placeholder: RERANKERS[0], style: { width: "220px" }, onchange: () => { x.model = m.value.trim(); changed(); } });
      parts.push(m, h("datalist", { id: "dz-rerankers" }, RERANKERS.map((y) => h("option", { value: y }))));
      more = h("div", { class: "dz-sub" }, h("span", { class: "small dim" }, "The text it reads of each row:"), h("div", { class: "row dz-chips" }, headFields("cross").map((f) => { const on = (x.fields || []).includes(f.name); return h("button", { class: "chip", "aria-pressed": String(on), onclick: () => { x.fields = on ? x.fields.filter((y) => y !== f.name) : [...(x.fields || []), f.name]; changed(); } }, f.name); }), !(x.fields || []).length ? h("span", { class: "small dim" }, "none picked: the stored words fields, else the text the rows show") : null));
    }
    parts.push(h("span", { class: "grow" }), h("span", { class: "small dim" }, "weight"), numIn(x.w ?? 1, (v) => { x.w = v ?? 0; changed({ redraw: false }); }, { step: 0.1 }), h("button", { class: "btn xs ghost", title: "Remove", onclick: remove }, icon("x")));
    return h("div", { class: "dz-head" }, h("div", { class: "row dz-frow", style: { flexWrap: "wrap" } }, parts), more);
  }

  function editBlending() {
    const b = d.blend;
    const kws = T.fieldsOf(t).filter((f) => (f.type === "keyword" || f.type === "int" || f.type === "bool") && f.name !== t.short?.key);
    const vecs = T.vectorFields(t);
    const others = usable().filter((x) => x.table !== t.table && T.textFields(x).length);
    const templates = saved.filter((x) => x.name !== d.name);
    const kinds = T.SOURCE_KINDS.filter(([k]) => k === "sql" || (k === "template" ? templates.length : others.length));
    const newSource = (k) => k === "template" ? { kind: k, template: templates[0]?.name || "", depth: 20, boost: 0 }
      : k === "sql" ? { kind: k, name: "query", sql: `SELECT *, score() FROM ${t.table} ORDER BY bm25(${d.words.fields[0]?.field || "body"}, :q, syntax => 'plain') DESC LIMIT 20`, boost: 0 }
      : { kind: "table", table: others[0]?.table, name: others[0]?.table, field: T.textFields(others[0])[0]?.name, depth: 20, boost: 0 };
    const boostIn = (x) => [h("span", { class: "small dim", title: "utility added to each of its rows: above 0 favours this query's rows, below 0 the others'" }, "boost"), numIn(x.boost ?? 0, (v) => { x.boost = v || 0; changed({ redraw: false }); }, { step: 0.05 })];
    const srcRows = h("div", { class: "stack", style: { gap: "6px" } },
      h("div", { class: "dz-head" }, h("div", { class: "row dz-frow" }, h("span", { class: "badge" }, "results"), h("span", { class: "small" }, "this template's ranked rows"), h("span", { class: "grow" }), boostIn(b))),
      b.sources.map((s, i) => {
        const kind = s.kind || "table";
        const st = kind === "table" ? tableOf(s.table) : null;
        const parts = [h("select", { class: "select sm", "aria-label": "Source kind", onchange: (e) => { b.sources[i] = { ...newSource(e.target.value), boost: s.boost || 0 }; changed(); } }, T.SOURCE_KINDS.map(([k, l]) => h("option", { value: k, selected: k === kind, disabled: !kinds.some((x) => x[0] === k) }, l)))];
        if (kind === "template") parts.push(h("select", { class: "select sm", "aria-label": "Template", onchange: (e) => { s.template = e.target.value; changed(); } }, templates.map((x) => h("option", { value: x.name, selected: x.name === s.template }, `${x.name} · ${x.design.table}`))), h("span", { class: "small dim" }, "top"), numIn(s.depth ?? 20, (v) => { s.depth = v || 20; changed({ redraw: false }); }, { min: 1 }));
        if (kind === "table") parts.push(h("select", { class: "select sm", "aria-label": "Table", onchange: (e) => { s.table = e.target.value; s.name = s.table; s.field = T.textFields(tableOf(s.table))[0]?.name || ""; changed(); } }, others.map((x) => h("option", { value: x.table, selected: x.table === s.table }, x.table))),
          st ? fieldSelect(T.textFields(st), s.field, (v) => { s.field = v; changed(); }, { label: "Its words field" }) : null,
          h("span", { class: "small dim" }, "top"), numIn(s.depth ?? 20, (v) => { s.depth = v || 20; changed({ redraw: false }); }, { min: 1 }));
        if (kind === "sql") parts.push(h("input", { class: "input sm mono", value: s.name || "", placeholder: "name", style: { width: "110px" }, "aria-label": "Source name", onchange: (e) => { s.name = e.target.value; changed({ redraw: false }); } }));
        parts.push(h("span", { class: "grow" }), boostIn(s), h("button", { class: "btn xs ghost", title: "Remove", onclick: () => { b.sources.splice(i, 1); changed(); } }, icon("x")));
        const sqlIn = kind === "sql" ? h("textarea", { class: "input", spellcheck: "false", "aria-label": "Its query", onchange: (e) => { s.sql = e.target.value; changed({ redraw: false }); } }, s.sql || "") : null;
        return h("div", { class: "dz-head dz-src" }, h("div", { class: "row dz-frow", style: { flexWrap: "wrap" } }, parts), sqlIn);
      }),
      h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } }, kinds.map(([k, l, tt]) => h("button", { class: "btn xs", title: tt, onclick: () => { b.sources.push(newSource(k)); changed(); } }, icon("plus"), l))),
      !templates.length ? h("span", { class: "small dim" }, "Saved templates show here once there is another one: save this one, start a new template, and blend them.") : null);
    return h("div", { class: "stack" },
      section("Blend the page", "after ranking, rows are placed slot by slot under rules; each placed row says why it is there",
        opt("Blend", "off: the ranked rows are the page", toggle(b.on, (v) => { b.on = v; changed(); })),
        b.on ? opt("Rows it reads", "the ranked rows the fill places from", numIn(b.depth, (v) => { b.depth = Math.max(1, Math.min(1000, v || 50)); changed({ redraw: false }); }, { min: 1, max: 1000 })) : null),
      b.on ? section("Queries blended", "each query runs with the same text; their rows are placed together on one page, each row saying which query it came from", srcRows) : null,
      b.on ? section("Rules", null,
        opt("Diversity", "a row loses this share of the utilities' range for its likeness to a row placed (0 to 1)", h("div", { class: "row nw" },
          fieldSelect([...kws, ...vecs], b.diversify?.on, (v) => { b.diversify = v ? { on: v, lambda: b.diversify?.lambda ?? 0.3 } : null; changed(); }, { none: "off", label: "Diversify on" }),
          b.diversify?.on ? numIn(b.diversify.lambda, (v) => { b.diversify.lambda = Math.max(0, Math.min(1, v ?? 0.3)); changed({ redraw: false }); }, { step: 0.05, min: 0, max: 1 }) : null)),
        opt("At most per value", "no more than n rows of one value: a brand, an author", h("div", { class: "row nw" },
          fieldSelect(kws, b.cap?.per, (v) => { b.cap = v ? { per: v, at_most: b.cap?.at_most ?? 2 } : null; changed(); }, { none: "off", label: "Cap per" }),
          b.cap?.per ? numIn(b.cap.at_most, (v) => { b.cap.at_most = Math.max(1, v || 2); changed({ redraw: false }); }, { min: 1 }) : null)),
        vecs.length ? opt("Near duplicates", "drop a row whose vector's cosine to a row placed is this or more", h("div", { class: "row nw" },
          fieldSelect(vecs, b.dedupe?.on, (v) => { b.dedupe = v ? { on: v, threshold: b.dedupe?.threshold ?? 0.95 } : null; changed(); }, { none: "off", label: "Dedupe on" }),
          b.dedupe?.on ? numIn(b.dedupe.threshold, (v) => { b.dedupe.threshold = Math.max(0, Math.min(1, v ?? 0.95)); changed({ redraw: false }); }, { step: 0.01, min: 0, max: 1 }) : null)) : null,
        opt("A share for a kind", "rows of a value between a and b percent of the top n", h("div", { class: "row", style: { flexWrap: "wrap", gap: "6px" } },
          fieldSelect(kws, b.quota?.column, (v) => { b.quota = v ? { column: v, value: b.quota?.value ?? "", min: b.quota?.min ?? 0, max: b.quota?.max ?? 30, top: b.quota?.top ?? null } : null; changed(); }, { none: "off", label: "Quota on" }),
          b.quota?.column ? [h("span", { class: "small dim" }, "="), h("input", { class: "input sm", value: b.quota.value ?? "", placeholder: "value", style: { width: "120px" }, oninput: (e) => { b.quota.value = e.target.value; changed({ redraw: false }); } }),
            h("span", { class: "small dim" }, "from %"), numIn(b.quota.min, (v) => { b.quota.min = v ?? 0; changed({ redraw: false }); }, { min: 0, max: 100 }), h("span", { class: "small dim" }, "to %"), numIn(b.quota.max, (v) => { b.quota.max = v ?? 100; changed({ redraw: false }); }, { min: 0, max: 100 }),
            h("span", { class: "small dim" }, "of the top"), numIn(b.quota.top, (v) => { b.quota.top = v || null; changed({ redraw: false }); }, { min: 1, placeholder: "all" })] : null))) : null,
      b.on ? h("div", { class: "small dim" }, "Slots kept for a kind, spacing and heads of their own are in ", h("a", { href: link("blend", d.savedVersion ? { name: `${d.name}_blend` } : {}) }, "Blending"), d.savedVersion ? ", where the saved blend opens." : ": save the template, then open its blend there.") : null);
  }

  function editSave() {
    const made = () => { try { return { stmts: T.saveStatements(d, t, { hadBlend: !!d.hadBlend }) }; } catch (e) { return { err: e }; } };
    let said = ""; try { said = T.describe({ ...d, about: "" }, t); } catch { /* the statements say why */ }
    // What the name changes is drawn again in place, not the whole step:
    // a click on Save that ends the name's typing still lands on Save.
    const shown = h("div", { class: "stack" });
    const about = h("input", { class: "input", value: d.about || "", placeholder: said, style: { width: "100%" }, oninput: () => { d.about = about.value; keep(); } });
    about.addEventListener("change", () => drawShown());
    const nameIn = h("input", { class: "input mono", value: d.name, placeholder: "product_search", spellcheck: "false", oninput: () => { d.name = T.slug(nameIn.value); keep(); drawStepper(); drawShown(); } });
    nameIn.addEventListener("change", () => drawTop());
    const reason = isProduction() ? h("input", { class: "input", placeholder: "Why: kept in the audit log", style: { width: "100%" } }) : null;
    const btn = h("button", { class: "btn primary" }, icon("bookmark"), d.savedVersion ? "Save a new version" : "Save the template");
    gate(btn, "admin");
    if (reason) { btn.disabled = true; btn.title = "Say why first"; reason.oninput = () => { btn.disabled = !reason.value.trim(); btn.title = btn.disabled ? "Say why first" : ""; }; }
    btn.onclick = () => saveIt(reason?.value.trim() || undefined, btn);
    const copyBtn = h("button", { class: "btn ghost", onclick: () => { const m = made(); if (m.err) toast(m.err.message, "bad"); else copy(m.stmts.map((s) => s.sql + ";").join("\n\n"), "Statements copied"); } }, icon("copy"), "Copy the statements");
    function drawShown() {
      const { stmts, err } = made();
      const exists = saved.some((s) => s.name === d.name) && !d.savedVersion;
      const taken = !saved.some((s) => s.name === d.name) && (fnNames.has(d.name) || fnNames.has(`${d.name}_template`));
      const call = `SELECT * FROM ${d.name || "name"}(:q)`;
      const open = new Set([...shown.querySelectorAll("details[open]")].map((x) => x.dataset.what));
      shown.replaceChildren(...[
        exists ? h("div", { class: "notice" }, icon("warn"), ` A template named ${d.name} exists: saving makes its next version (the old ones stay, and can come back).`) : null,
        taken ? h("div", { class: "notice" }, icon("warn"), ` A stored function named ${fnNames.has(d.name) ? d.name : d.name + "_template"} exists and is not a template: saving replaces it with a new version. Pick another name to keep it as it is.`) : null,
        err ? h("div", { class: "error" }, err.message) : section("What saving runs", "each a CREATE statement the engine keeps, versions and ships to replicas and shards",
          h("div", { class: "stack", style: { gap: "8px" } }, stmts.map((s) => h("details", { class: "dz-stmt", open: open.size ? open.has(s.what) : s.what === "template", "data-what": s.what },
            h("summary", {}, h("b", {}, { ranking: "The ranking, as a rank profile", blend: s.sql.startsWith("DROP") ? "The blend, dropped (blending is off)" : "The page, as a blend", template: "The template, as a function apps call", design: "The design, so this page opens it again" }[s.what]),
              h("span", { class: "small dim" }, { ranking: ` · rank_profile('${d.name}'): the relevance autopilot can tune its weights`, blend: s.sql.startsWith("DROP") ? "" : ` · blend('${d.name}_blend')`, template: ` · ${call}, and the MCP tool fn_${d.name}`, design: ` · ${d.name}_template()` }[s.what])),
            sqlBlock(s.sql))))),
      ].filter(Boolean));
    }
    drawShown();
    return h("div", { class: "stack" },
      h("div", { class: "row", style: { gap: "10px", flexWrap: "wrap" } }, h("label", { class: "f", style: { minWidth: "220px" } }, "Name", nameIn), h("label", { class: "f grow" }, "What it finds, for people and agents", about)),
      shown,
      reason ? h("label", { class: "f" }, "Why", reason) : null,
      h("div", { class: "row", style: { gap: "8px" } }, btn, copyBtn),
      d.savedVersion ? section("Use it", "the same search from anywhere: the text is its one input",
        useFromCode({ sql: `SELECT * FROM ${d.name}(q => :q)`, params: { q: sample || "red shoes" }, tool: `fn_${d.name}` }),
        h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } },
          h("a", { class: "btn sm", href: `#/saved/${encodeURIComponent(d.name)}` }, icon("bookmark"), "Its saved search"),
          h("a", { class: "btn sm", href: link("funnel", { tab: "one", sql: `SELECT * FROM ${d.name}(q => :q)`, p: JSON.stringify({ q: sample || "" }) }) }, icon("filter"), "Its funnel"),
          can("admin") ? h("a", { class: "btn sm", href: "#/autopilot" }, icon("relevance"), "Tune its ranking") : null,
          d.blend.on ? h("a", { class: "btn sm", href: link("blend", { name: `${d.name}_blend` }) }, icon("sliders"), "Its blend") : null)) : null);
  }

  async function saveIt(reason, btn) {
    if (!d.name) { toast("Name the template first", "bad"); return; }
    let stmts;
    if (sample && !d.samples.includes(sample)) d.samples.unshift(sample);
    // The design saved is the one the statements were made of, with its samples.
    try { stmts = T.saveStatements(d, t, { hadBlend: !!d.hadBlend }); } catch (e) { toast(e.message, "bad"); return; }
    btn.disabled = true;
    const done = [];
    try {
      for (const s of stmts) { await api("/v1/sql", { body: { sql: s.sql }, reason }); done.push(s.what); }
    } catch (e) {
      btn.disabled = false;
      toast(`${done.length ? `Saved the ${done.join(", ")}, then ` : ""}the ${stmts[done.length].what} failed: ${e.message}`, "bad", 8000);
      return;
    }
    d.hadBlend = d.blend.on;
    await loadSaved();
    const s = saved.find((x) => x.name === d.name);
    d.savedVersion = s?.version || 1;
    keep();
    toast(`Saved ${d.name}: SELECT * FROM ${d.name}('…') runs it`, "ok");
    drawAll();
  }

  // ------------------------------------------------------------ the preview
  /** Whether a step's preview needs more than the funnel. */
  const needs = (id) => id !== "save";
  async function runPreview() {
    if (!d || !t) return;
    const my = ++seq;
    const text = (sample || "").trim();
    if (!text) { pv = {}; drawPreview(); drawStepper(); return; }
    let full, params, first;
    try { full = T.querySql(d, t); params = T.params(d, t, text); first = T.querySql(d, t, { upTo: "retrieval" }); }
    catch (e) { pv = { error: e }; drawPreview(); drawStepper(); return; }
    preview.classList.add("busy");
    const q = (sql, p = params) => api("/v1/query", { body: { sql, params: p } }).catch((e) => ({ error: e }));
    const jobs = {};
    jobs.funnel = api("/v1/funnel", { body: { sql: full, params } }).catch((e) => ({ error: e }));
    const primary = d.words.on && d.words.fields[0]?.field;
    if (step === "understanding") {
      if (primary) jobs.correct = api("/v1/correct", { body: { table: t.table, field: primary, text } }).catch((e) => ({ error: e }));
      if (primary && d.understand.values && d.understand.fields.length) jobs.understood = api("/v1/understand", { body: { table: t.table, field: primary, text, fields: d.understand.fields.join(", "), mode: d.understand.mode } }).catch((e) => ({ error: e }));
      const off = { ...d, understand: { ...d.understand, values: false } };
      try { jobs.without = q(T.querySql(off, t, { upTo: "retrieval" })); jobs.with = q(first); } catch { /* the editor says why */ }
    }
    if (step === "retrieval") {
      jobs.with = q(first);
      const ins = T.inputs(d, t);
      if (ins.length > 1) {
        jobs.byInput = Promise.all(ins.map((x) => {
          const one = { ...d, words: { ...d.words, on: x.kind === "words", fields: x.kind === "words" ? [{ field: x.field, weight: 1 }] : [] }, meaning: { ...d.meaning, on: x.kind === "meaning" }, rank: { boosts: [], rerank: { on: false } } };
          return q(T.querySql(one, t, { upTo: "retrieval" })).then((j) => ({ input: x, j }));
        }));
      }
    }
    if (step === "ranking") {
      jobs.with = q(first, { q: text });
      const hd = d.rank.heads;
      if (hd.on && hd.list.length > 1) {
        jobs.byHead = Promise.all(hd.list.map((x) => {
          let sql;
          try { sql = T.querySql({ ...d, rank: { ...d.rank, heads: { ...hd, list: [{ ...x, w: 1 }] } } }, t); } catch (e) { return { head: x, j: { error: e } }; }
          return q(sql).then((j) => ({ head: x, j }));
        }));
      }
    }
    if (step === "blending" && d.blend.on) jobs.blend = api("/v1/blends/preview", { method: "POST", body: { def: T.blendDef(d, t), params, costs: true } }).catch((e) => ({ error: e }));
    else if (d.blend.on && pv.blend && pv.blendFor === JSON.stringify(d.blend)) jobs.blend = Promise.resolve(pv.blend);
    const keys = Object.keys(jobs);
    const vals = await Promise.all(keys.map((k) => jobs[k]));
    if (my !== seq || !alive) return;
    pv = Object.fromEntries(keys.map((k, i) => [k, vals[i]]));
    pv.text = text; pv.sql = full; pv.params = params; pv.step = step;
    if (pv.blend) pv.blendFor = JSON.stringify(d.blend);
    preview.classList.remove("busy");
    drawPreview(); drawStepper();
  }

  function drawPreview() {
    if (!d) return;
    preview.classList.remove("busy");
    const text = (sample || "").trim();
    if (!text) { clear(preview, card({ body: empty("search", "Type a sample query", "Each stage shows what it does to it. The preview runs queries; nothing is stored.") })); clear(funnelBox); return; }
    if (pv.error) { clear(preview, card({ body: h("div", { class: "error" }, pv.error.message) })); clear(funnelBox); return; }
    if (!pv.funnel) { clear(preview, card({ body: live ? h("div", { class: "small dim" }, "Previewing…") : empty("play", "Not previewed yet", "Press Preview to run the sample through every stage.", h("button", { class: "btn primary", onclick: runPreview }, icon("play"), "Preview")) })); clear(funnelBox); return; }
    const f = pv.funnel;
    const body = { understanding: showUnderstanding, retrieval: showRetrieval, ranking: showRanking, blending: showBlending, save: showSave }[step]();
    const stale = pv.step !== step && step !== "save";
    const viewSeg = seg([["flow", "Flow", "each row's move drawn and colored by how far it went"], ["lists", "Lists", "the lists side by side, with arrows"]], view, (v) => { view = v; store.set("designer.view", v); drawPreview(); });
    viewSeg.classList.add("dz-view");
    clear(preview, card({ title: `What ${STEPS.find((x) => x[0] === step)[1].toLowerCase()} does to “${clip(pv.text, 40)}”`, sub: stale ? "previewing…" : "a preview: nothing is stored", body: [h("div", { class: "row dz-view-row" }, h("span", { class: "small dim" }, "Show changes as"), viewSeg), body] }));
    // The funnel: the rows each stage kept, as the Query funnel draws it.
    if (f.error) clear(funnelBox, card({ body: failure(f.error, "The funnel") }));
    else if (f.rows && f.rows.kind !== "other") {
      const fc = rowsFunnel(f.rows, [...(f.rows.notes || []), ...(f.notes || [])]);
      if (d.blend.on && pv.blend?.rows) fc.querySelector(".fn-funnel")?.append(h("div", { class: "small dz-blendline" }, icon("sliders"), ` Blend placed ${fmt.plural(pv.blend.rows.length, "row")} on the page${pv.blend.passed?.length ? `, passed over ${pv.blend.passed.length}` : ""}`));
      clear(funnelBox, fc, h("div", { class: "row small" }, h("span", { class: "dim" }, `Took ${fmt.ms(f.took_ms)} on the server.`), h("a", { href: link("funnel", { tab: "one", sql: pv.sql, p: JSON.stringify(pv.params) }) }, "Open it in Query funnel", icon("right"))));
    } else clear(funnelBox, card({ body: h("div", { class: "small dim" }, "This query's rows are not traced stage by stage: its time is in Query funnel.") }));
  }

  /** A short ranked list of an answer's rows: [key, display], marking those not in `other`. */
  /** How a row's columns read as a result: [key, title, line, facts, score], by the table's fields. */
  const NOT_SHOWN = new Set(["score()", "score", "slot", "source", "utility", "blend_decision"]);
  function shape(columns) {
    const cs = columns || [];
    const key = t?.short?.key;
    const ki = Math.max(0, cs.indexOf(key));
    const isText = (c) => F(c)?.type === "text";
    const texts = cs.map((c, i) => [c, i]).filter(([c, i]) => i !== ki && isText(c));
    const ti = texts.find(([c]) => /^(title|name|subject|headline)$/i.test(c))?.[1] ?? texts[0]?.[1] ?? cs.findIndex((c, i) => i !== ki && !NOT_SHOWN.has(c));
    const li = texts.find(([, i]) => i !== ti)?.[1] ?? -1;
    const facts = cs.map((c, i) => [c, i]).filter(([c, i]) => i !== ki && i !== ti && i !== li && !NOT_SHOWN.has(c) && F(c)?.type !== "vector");
    const sc = cs.findIndex((c) => c === "score()" || c === "score");
    return { ki, ti, li, facts, sc };
  }
  /** A fact of a row as people read it: a price with cents, a rating with a star, a date as a date. */
  function fact(c, v) {
    if (v == null || v === "") return null;
    const f = F(c);
    if (Array.isArray(v)) return v.slice(0, 3).join(", ");
    if (f?.type === "timestamp") { const dt = new Date(typeof v === "number" ? v : String(v)); return Number.isNaN(dt.getTime()) ? String(v) : dt.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }); }
    if (typeof v === "number" && /price|cost|amount/i.test(c)) return v.toFixed(2);
    if (typeof v === "number" && /rating|stars/i.test(c)) return `★ ${+v.toFixed(1)}`;
    if (typeof v === "number" && /^(sold|reviews|views|clicks|count)/i.test(c)) return `${fmt.si(v)} ${c}`;
    return String(v);
  }
  /** A row as a result: its title, a line of its text (full only) and its facts. */
  function result(cols, x, sh, { full = false } = {}) {
    const title = sh.ti >= 0 ? x[sh.ti] : x[sh.ki];
    const facts = sh.facts.map(([c, i]) => { const v = fact(c, x[i]); return v == null ? null : h("span", { title: c }, v); }).filter(Boolean);
    return h("div", { class: "dz-res" },
      h("div", { class: "dz-res-t", title: `${t?.short?.key || "key"} ${keyText(x[sh.ki])}` }, clip(title, full ? 120 : 64)),
      full && sh.li >= 0 && x[sh.li] ? h("div", { class: "dz-res-l" }, clip(x[sh.li], 160)) : null,
      facts.length ? h("div", { class: "dz-res-f" }, facts.flatMap((e, i) => (i ? [h("span", { class: "dim" }, " · "), e] : [e]))) : null);
  }
  /** A ranked list of an answer's rows as results, marking those not in `other` and how far each moved. */
  function list(j, { other = null, moves = null, full = false } = {}) {
    if (j?.error) return failure(j.error, "The query");
    if (!j?.rows?.length) return h("div", { class: "small dim" }, "No row.");
    const sh = shape(j.columns);
    return h("ol", { class: "dz-rows dz-results" + (full ? " full" : "") }, j.rows.map((x, i) => {
      const k = keyText(x[sh.ki]);
      const mv = moves?.get(k);
      const dm = moves ? (mv == null ? "new" : mv - i) : null;
      const tint = moves && view === "flow";
      return h("li", { class: (other && !other.has(k) ? "new" : "") + (tint ? " mv" : ""), style: tint ? { "--mv": moveColor(dm), "--mv-bg": moveColor(dm, 0.12) } : null },
        h("span", { class: "dz-rank num" }, String(i + 1)),
        result(j.columns, x, sh, { full }),
        h("span", { class: "dz-side-n" },
          moves ? h("span", { class: view === "flow" ? "dz-mv-chip" : "dz-mv " + (mv == null ? "in" : mv > i ? "up" : mv < i ? "down" : ""), title: mv == null ? "not in the list before" : `was #${mv + 1}` }, mv == null ? "new" : mv > i ? `↑${mv - i}` : mv < i ? `↓${i - mv}` : view === "flow" ? "=" : "·") : null,
          sh.sc >= 0 && x[sh.sc] != null ? h("span", { class: "small muted num dz-score", title: "score" }, fmt.num(+Number(x[sh.sc]).toPrecision(3))) : null));
    }));
  }
  /** A row's move as a color: green up, red down, stronger the farther; blue for a row new to the list, grey for one it lost. */
  function moveColor(d, alpha = 1) {
    if (d === "new") return `rgba(37, 99, 235, ${0.85 * alpha})`;
    if (d === "gone") return `rgba(120, 128, 140, ${0.6 * alpha})`;
    if (!d) return `rgba(120, 128, 140, ${0.35 * alpha})`;
    const a = (0.3 + 0.7 * Math.min(1, Math.abs(d) / 6)) * alpha;
    return d > 0 ? `rgba(22, 163, 74, ${a})` : `rgba(220, 38, 38, ${a})`;
  }
  const moveTitle = (d) => d === "new" ? "new: not in the list before" : d === "gone" ? "left out after this step" : !d ? "same place" : d > 0 ? `up ${d}` : `down ${-d}`;
  /**
   * How a step reorders rows: the list before on the left, after on the
   * right, a line between each row's two places colored by how far it
   * moved (green up, red down, stronger the farther), rows new to the
   * list in blue and rows it lost in grey.
   */
  function flow(a, b, { from = "Before", to = "After", n = 10 } = {}) {
    if (a?.error || b?.error) return failure(a?.error || b?.error, "The comparison");
    if (view === "lists") return h("div", { class: "dz-cmp" },
      h("div", {}, h("b", { class: "small" }, from), list({ columns: a.columns, rows: (a.rows || []).slice(0, n) }, { other: ids(b) })),
      h("div", {}, h("b", { class: "small" }, to), list({ columns: b.columns, rows: (b.rows || []).slice(0, n) }, { other: ids(a), moves: positions(a) })));
    const A = (a?.rows || []).slice(0, n), B = (b?.rows || []).slice(0, n);
    if (!A.length && !B.length) return null;
    const sa = shape(a.columns), sb = shape(b.columns);
    const ka = A.map((x) => keyText(x[sa.ki])), kb = B.map((x) => keyText(x[sb.ki]));
    const pa = new Map(ka.map((k, i) => [k, i])), pb = new Map(kb.map((k, i) => [k, i]));
    const ROW = 30, W = 64, rows = Math.max(A.length, B.length), H = rows * ROW;
    const y = (i) => i * ROW + ROW / 2;
    const lines = [];
    ka.forEach((k, i) => {
      const j = pb.get(k);
      if (j == null) lines.push(svg("path", { d: `M0 ${y(i)} C ${W * 0.5} ${y(i)}, ${W * 0.6} ${H + 6}, ${W * 0.7} ${H + 6}`, stroke: moveColor("gone"), "stroke-width": 1.5, "stroke-dasharray": "3 3", fill: "none" }));
      else lines.push(svg("path", { d: `M0 ${y(i)} C ${W / 2} ${y(i)}, ${W / 2} ${y(j)}, ${W} ${y(j)}`, stroke: moveColor(i - j), "stroke-width": 1.5 + Math.min(3, Math.abs(i - j) / 2), fill: "none" }, svg("title", {}, moveTitle(i - j))));
    });
    kb.forEach((k, j) => { if (!pa.has(k)) lines.push(svg("path", { d: `M${W * 0.3} -6 C ${W * 0.4} -6, ${W * 0.5} ${y(j)}, ${W} ${y(j)}`, stroke: moveColor("new"), "stroke-width": 2, "stroke-dasharray": "3 3", fill: "none" })); });
    const side = (R, keys, other, sh, left) => h("ol", { class: "dz-flow-col" + (left ? " l" : " r") }, R.map((x, i) => {
      const o = other.get(keys[i]);
      const d = o == null ? (left ? "gone" : "new") : left ? i - o : o - i;
      const title = sh.ti >= 0 ? x[sh.ti] : keys[i];
      return h("li", { style: { "--mv": moveColor(d), "--mv-bg": moveColor(d, 0.14), height: `${ROW}px` }, title: `${clip(title, 200)} · ${moveTitle(d)}` },
        h("span", { class: "dz-rank num" }, String(i + 1)), h("span", { class: "dz-flow-t" }, clip(title, 60)),
        !left && d !== 0 ? h("span", { class: "dz-flow-d" }, d === "new" ? "new" : d > 0 ? `↑${d}` : `↓${-d}`) : null);
    }));
    const moved = kb.filter((k, j) => pa.has(k) && pa.get(k) !== j).length, fresh = kb.filter((k) => !pa.has(k)).length;
    return h("div", { class: "dz-flow" },
      h("div", { class: "dz-flow-h" }, h("b", { class: "small" }, from), h("span"), h("b", { class: "small" }, to)),
      h("div", { class: "dz-flow-b" }, side(A, ka, pb, sa, true), svg("svg", { width: W, height: H + 8, viewBox: `0 -6 ${W} ${H + 14}`, class: "dz-flow-svg", "aria-hidden": "true" }, lines), side(B, kb, pa, sb, false)),
      h("div", { class: "small dim dz-flow-k" },
        h("span", { class: "dz-key", style: { "--mv": moveColor(4) } }), "up", h("span", { class: "dz-key", style: { "--mv": moveColor(-4) } }), "down (stronger the farther)",
        h("span", { class: "dz-key", style: { "--mv": moveColor("new") } }), "new", h("span", { class: "dz-key", style: { "--mv": moveColor("gone") } }), "left out",
        h("span", { class: "grow" }), `${moved} of ${kb.length} moved, ${fresh} new`));
  }
  const keyAt = (j) => shape(j?.columns).ki;
  const ids = (j) => new Set((j?.rows || []).map((x) => keyText(x[keyAt(j)])));
  const positions = (j) => new Map((j?.rows || []).map((x, i) => [keyText(x[keyAt(j)]), i]));

  function showUnderstanding() {
    const out = [];
    const c = pv.correct, u = pv.understood;
    out.push(h("div", { class: "dz-read" }, h("b", { class: "small" }, "Did you mean"),
      !c ? h("span", { class: "small dim" }, "–") : c.error ? failure(c.error, "Correction") : c.changed ? h("span", {}, h("b", {}, c.text), h("span", { class: "small dim" }, ` · ${c.words.map((w) => `${w.word} → ${w.to_word}`).join(", ")}${d.words.typos === "off" ? " · typo tolerance is off, so the query searches the words as typed" : " · the typo tolerance finds these"}`)) : h("span", { class: "small dim" }, "every word is in the field, or no close word is likely enough")));
    if (d.understand.values) {
      out.push(h("div", { class: "dz-read" }, h("b", { class: "small" }, "Values named"),
        !u ? h("span", { class: "small dim" }, "pick the fields whose values a query may name") : u.error ? failure(u.error, "Understanding") : h("div", { class: "stack", style: { gap: "4px" } },
          (u.spans || []).length || (u.ranges || []).length ? h("div", { class: "row dz-chips" },
            ...(u.spans || []).flatMap((sp) => sp.matches.slice(0, 4).map((m) => h("span", { class: "chip", title: `“${sp.words}”: ${m.rows} rows` }, `${m.field} = ${m.value}`))),
            ...(u.ranges || []).map((r) => h("span", { class: "chip", title: `“${r.words}”` }, `${r.field} ${[r.lo ? `${r.lo[1] ? "≥" : ">"} ${r.lo[0]}` : null, r.hi ? `${r.hi[1] ? "≤" : "<"} ${r.hi[0]}` : null].filter(Boolean).join(" and ")}`))) : h("span", { class: "small dim" }, `no word names a value of ${d.understand.fields.join(", ")}`),
          h("span", { class: "small" }, "Searched as words: ", h("b", {}, u.ranked || "(nothing left)")))));
    }
    const lines = pv.funnel?.text?.understood;
    if (lines?.length) out.push(h("div", { class: "stack", style: { gap: "2px" } }, lines.map((l) => h("div", { class: "mono small dim" }, l))));
    if (pv.with && pv.without) out.push(flow(pv.without, pv.with, { from: "Without understanding", to: d.understand.values ? "With it" : "With it (off)" }));
    return h("div", { class: "stack" }, out);
  }

  function showRetrieval() {
    const f = pv.funnel, rows = f?.rows;
    const out = [];
    if (rows?.inputs?.length) out.push(h("div", { class: "dz-inputs" }, rows.inputs.map((i) => h("div", { class: "dz-in" },
      h("b", { class: "mono small" }, i.label),
      h("span", { class: "small" }, i.matched != null ? `${fmt.plural(i.matched, "row")} match` : "ranks every row with a vector"),
      i.candidates != null ? h("span", { class: "small dim" }, `${fmt.n(i.candidates)} of its top rows are candidates`) : null))));
    for (const s of f?.text?.searches || []) out.push(termsOf(s));
    if (f?.text?.expanded?.length) out.push(h("div", { class: "stack", style: { gap: "2px" } }, h("b", { class: "small" }, "Feedback from the first rows"), f.text.expanded.map((l) => h("div", { class: "mono small dim" }, l))));
    if (f?.text?.embedded?.length) out.push(h("div", { class: "small dim" }, f.text.embedded.map((e) => `embedded “${clip(e.text, 40)}” for ${e.field || e.model}${e.ms != null ? ` in ${fmt.ms(e.ms)}` : ""}`).join(" · ")));
    if (pv.byInput?.length) {
      for (const { input, j } of pv.byInput) out.push(flow(j, pv.with, { from: `${input.label} alone`, to: d.fusion === "rrf" ? "Fused by rank" : "Fused by weights" }));
    } else if (pv.with) out.push(h("div", {}, h("b", { class: "small" }, "The retrieval's top rows"), list(pv.with, { full: true })));
    if (f?.error) out.push(failure(f.error, "The funnel"));
    return h("div", { class: "stack" }, out);
  }

  function showRanking() {
    const ans = pv.funnel?.answer;
    const out = [];
    const rows = pv.funnel?.rows;
    if (rows?.phases?.length) out.push(h("div", { class: "stack", style: { gap: "2px" } }, rows.phases.map((p) => h("div", { class: "small" }, h("span", { class: "mono" }, clip(p.text, 110)), h("span", { class: "dim" }, ` · ${fmt.n(p.count)} rows ranked again`)))));
    if (!d.rank.boosts.length && !(d.rank.heads.on && d.rank.heads.list.length)) out.push(h("div", { class: "notice" }, "No boost and no head yet: the page is the retrieval's order. Add one on the left to see which rows it moves."));
    if (ans && pv.with) out.push(flow(pv.with, { columns: ans.columns, rows: ans.rows }, { from: "Retrieved", to: "Ranked" }));
    if (ans) {
      const was = pv.with ? positions(pv.with) : null;
      out.push(h("div", {}, h("b", { class: "small" }, "The page, as a person sees it"), h("span", { class: "small dim" }, was ? " · arrows: how far each row moved from the retrieval's order" : ""), list({ columns: ans.columns, rows: ans.rows }, { moves: was, full: true })));
    }
    if (pv.byHead?.length) {
      const page = new Set((ans?.rows || []).map((x) => keyText(x[keyAt(ans)])));
      out.push(h("b", { class: "small" }, "Each head alone, over the same rows"),
        h("div", { class: "dz-cmp three" }, pv.byHead.map(({ head, j }) => h("div", {}, h("b", { class: "small" }, T.headLabel(head), h("span", { class: "dim" }, ` · weight ${head.w ?? 1}`)), list(j, { other: page })))),
        h("div", { class: "small dim" }, "Highlighted: rows a head puts in its top that the fused heads leave off the page."));
    }
    if (pv.funnel?.error) out.push(failure(pv.funnel.error, "The ranking"));
    return h("div", { class: "stack" }, out);
  }

  function showBlending() {
    if (!d.blend.on) {
      const ans = pv.funnel?.answer;
      return h("div", { class: "stack" }, h("div", { class: "notice" }, "Blending is off: the page is the ranked rows. Turn it on to spread them, cap them, or mix in other tables."), ans ? list({ columns: ans.columns, rows: ans.rows }, { full: true }) : null);
    }
    const b = pv.blend;
    if (!b) return h("div", { class: "small dim" }, "Blending…");
    if (b.error) return failure(b.error, "The blend");
    const rows = b.rows || [];
    const costs = (b.costs || []).filter((x) => x && x.share > 0);
    const sh = shape(b.columns);
    const cellsOf = (x) => Array.isArray(x.cells) ? x.cells : (b.columns || []).map((c) => x.cells?.[c]);
    const ans = pv.funnel?.answer;
    return h("div", { class: "stack" },
      ans ? flow({ columns: ans.columns, rows: ans.rows }, { columns: b.columns, rows: rows.map(cellsOf) }, { from: "Ranked", to: "Blended page" }) : null,
      h("ol", { class: "dz-rows dz-results full dz-slate" }, rows.map((x, i) => h("li", { title: x.why || "" },
        h("span", { class: "dz-rank num" }, String(i + 1)),
        x.cells && typeof x.cells === "object" ? result(b.columns, cellsOf(x), sh, { full: true }) : h("span", { class: "mono small" }, clip(keyText(x.key), 40)),
        h("span", { class: "dz-side-n" }, h("span", { class: "badge", title: `from the query ${x.source}` }, x.source), h("span", { class: "small dim dz-why" }, clip(x.why || "", 40)))))),
      b.passed?.length ? h("details", {}, h("summary", { class: "small" }, `Passed over: ${b.passed.length}`), h("ul", { class: "dz-rows" }, b.passed.map((x) => h("li", {}, h("span", { class: "mono small" }, clip(keyText(x.key), 22)), h("span", { class: "small dim" }, " " + clip(x.why, 90)))))) : null,
      costs.length ? h("div", { class: "small dim" }, "What each rule gives up of the page's utility: ", costs.map((c) => `${c.rule} ${fmt.pct(c.share)}${c.moved ? ` (${fmt.plural(c.moved, "row")} moved)` : ""}`).join(" · ")) : h("div", { class: "small dim" }, "No rule cost the page anything: the ranked order already meets them."));
  }

  function showSave() {
    const ans = pv.funnel?.answer;
    return h("div", { class: "stack" },
      h("div", { class: "small dim" }, d.blend.on ? "Saved, a call answers the blended page:" : "Saved, a call answers these rows:"),
      d.blend.on && pv.blend?.rows ? showBlending() : ans ? list({ columns: ans.columns, rows: ans.rows }, { full: true }) : null,
      h("div", { class: "small dim" }, "The query this preview runs, written out:"), sqlBlock(pv.sql || ""));
  }

  return () => { alive = false; reopen = null; };
}

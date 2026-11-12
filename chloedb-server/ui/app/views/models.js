// Models: the model behind each vector field, and how its vectors are
// made, stored and searched. Pick another model or technique for a field
// and the page changes it without stopping searches:
//
//   1. ALTER TABLE t ADD COLUMN <f>_next ... OPTIONS(embed = .., model = ..)
//      adds the field as chosen beside the old one; the server fetches a
//      model it knows, and fills in the vectors of the rows already there
//      (the backfill, in /v1/tables' embeddings);
//   2. the same searches on both fields, side by side, to compare;
//   3. ALTER TABLE t RENAME COLUMN <f> TO <f>_prev, RENAME COLUMN <f>_next
//      TO <f> switches: queries that name <f> use the new vectors (text
//      given for a vector is embedded by the field's model);
//   4. DROP COLUMN <f>_prev frees the old vectors (a single server; a
//      coordinator keeps the column), or the rename back undoes it.
//
// A field's model cannot change in place: its vectors are the model's
// (a fingerprint of the weights is kept with the field).
//
// The models: GET /v1/models lists those a field may name, how each makes
// its vectors (`technique`: static, transformer, late-interaction, ...),
// its table's precision and size; POST /v1/admin/models/fetch fetches one
// the server knows, POST /v1/admin/models/convert writes a static model's
// table again in int8 (or F16) as a model of its own.

import { h, icon, clear, fmt, api, toast, confirm, card, empty, failure, pill, seg, sqlBlock, drawer, gate, every, store, toggle, table as coreTable } from "../core.js";
import { tables, invalidate } from "../catalog.js";
import { quoteName } from "../sqledit.js";

export default {
  id: "models", title: "Models", icon: "cpu", group: "Data", range: false,
  keywords: "models model embedding embeddings embedder vector vectors field technique static model2vec potion int8 quantize quantized bert transformer colbert late interaction multi-vector hosted openai hnsw ivf flat sq8 binary rabitq codes backfill change switch compare fetch",
  render,
};

/** How each technique makes its vectors, in the order a picker shows them. */
export const TECHNIQUES = {
  static: { label: "Static", color: "var(--s1)", what: "Model2Vec: a vector per token, averaged. No neural network runs, so a text embeds in well under a millisecond on a CPU, at ingest and at query time." },
  transformer: { label: "Transformer", color: "var(--s5)", what: "BERT-style encoder: better at meaning than a static model, but each text runs the network: slower ingest and some milliseconds a query." },
  "late-interaction": { label: "Late interaction", color: "var(--s8)", what: "ColBERT: a vector for each token, kept in a multi-vector field and scored with maxsim. The most exact match of words and meaning; best as a rerank over BM25's best rows." },
  hosted: { label: "Hosted API", color: "var(--s3)", what: "Computed by a provider (OpenAI, Cohere, Voyage, ...) with its key in the server's environment: no model here, but a request a batch, with its latency and cost." },
  "image-text": { label: "Images and text", color: "var(--s2)", what: "SigLIP: images and text in one space, for a MEDIA field searched by words." },
  "sound-text": { label: "Sound and text", color: "var(--s4)", what: "CLAP: sound and text in one space." },
  hash: { label: "Test", color: "var(--muted)", what: "Hashed words, for tests: no meaning." },
  unknown: { label: "Other", color: "var(--muted)", what: "A model directory this page does not know." },
};
const tech = (t) => TECHNIQUES[t] || TECHNIQUES.unknown;

/** The ways a vector field is searched. */
const INDEXES = [["hnsw", "HNSW", "A graph: fast approximate search, the default"], ["flat", "Flat", "Every row scored: exact, for small tables"], ["ivf", "IVF", "Clusters: less memory than HNSW, a list of clusters searched"]];
/** The codes a vector field keeps beside its float vectors. */
const CODES = [
  ["sq8", "SQ8", "8 bits a dimension (the default): a quarter of the floats, near-exact scores"],
  ["binary", "Binary", "1 bit a dimension: 1/32 of the floats, more candidates rescored"],
  ["rabitq4", "RaBitQ 4", "4 bits a dimension, with a correction a row: half of SQ8"],
  ["rabitq2", "RaBitQ 2", "2 bits a dimension"],
  ["rabitq1", "RaBitQ 1", "1 bit a dimension, with a correction a row"],
];

const NEXT = "_next", PREV = "_prev";

/** The model and technique of a field, from its short JSON. */
function techniqueOf(f, models) {
  const m = models.find((x) => x.name.toLowerCase() === String(f.embed?.model || "").toLowerCase());
  const index = typeof f.index === "string" ? f.index : f.index ? Object.keys(f.index)[0] : "hnsw";
  const params = f.index && typeof f.index === "object" ? Object.values(f.index)[0] : null;
  return { model: m, technique: m?.technique || (f.embed ? "unknown" : null), index, params, codes: f.codes || "sq8", multi: !!f.multi, keep_f32: !!f.keep_f32 };
}

function techBadge(t, precision) {
  const T = tech(t);
  return h("span", { class: "badge mm-tech", style: { "--c": T.color }, title: T.what }, T.label + (precision && precision !== "F32" ? " · " + (precision === "I8" ? "int8" : precision.toLowerCase()) : ""));
}

function indexText(t) {
  const p = t.params || {};
  if (t.index === "hnsw") return `HNSW${p.m && p.m !== 16 ? ` m ${p.m}` : ""}${p.ef_construction && p.ef_construction !== 200 ? ` ef ${p.ef_construction}` : ""}`;
  if (t.index === "ivf") return `IVF ${p.nlist || ""}`.trim();
  return "Flat";
}

/** A field's vectors: whether its model is open here, and its backfill. */
function vectorsState(e, fetches) {
  if (!e) return h("span", { class: "muted small" }, "–");
  // A model the server knows and lacks: it is fetching it.
  if (e.error && fetches && /fetch it first|not fetched/.test(e.error)) return h("span", { title: e.error }, pill("pending", "fetching the model"));
  if (e.error) return h("span", { class: "row nw", title: e.error }, pill("bad", "model not open"));
  const b = e.backfill;
  if (b?.state === "running") return h("span", { class: "row nw" }, pill("pending", "filling in"), h("span", { class: "small dim num" }, `${fmt.n(b.embedded)} of ${fmt.n(b.missing)}`));
  if (b?.state === "failed") return h("span", { title: b.error }, pill("bad", `stopped after ${fmt.n(b.embedded)}`));
  return e.loaded ? pill("ok", "ready") : pill("pending", "model opening");
}

function render(page) {
  const head = h("div");
  const body = h("div", { class: "stack" });
  page.append(head, body);
  let tab = store.get("models.tab", "fields");
  let ts = [], models = [], dir = "", fetches = false, alive = true;
  const tabs = seg([["fields", "Fields"], ["models", "Models"]], tab, (v) => { tab = v; store.set("models.tab", v); draw(); });

  clear(head, h("div", { class: "page-head" },
    h("div", {}, h("h1", {}, "Models"), h("p", {}, "The model behind each vector field, and how its vectors are made, stored and searched")),
    h("div", { class: "tools" }, tabs, h("button", { class: "btn", title: "Read the tables and models again", onclick: () => load(true) }, icon("refresh"), "Refresh"))));

  async function load(force = false) {
    try {
      const [t, m] = await Promise.all([tables(force), api("/v1/models")]);
      ts = t; models = m.models || []; dir = m.dir || ""; fetches = !!m.fetches;
    } catch (e) { clear(body, failure(e, "The models")); return; }
    if (alive) draw();
  }

  function draw() {
    tabs.set(tab);
    clear(body, tab === "fields" ? fieldsTab() : modelsTab());
  }

  // ---------------------------------------------------------- fields
  function fieldsTab() {
    const rows = [], bare = [];
    for (const t of ts) {
      const s = t.short || {};
      const fields = s.fields || [];
      const names = new Set(fields.map((f) => f.name));
      for (const f of fields) {
        if (f.type !== "vector") continue;
        // The halves of a change are shown with the field they change.
        if (f.name.endsWith(NEXT) && names.has(f.name.slice(0, -NEXT.length))) continue;
        if (f.name.endsWith(PREV) && names.has(f.name.slice(0, -PREV.length))) continue;
        const next = fields.find((x) => x.name === f.name + NEXT);
        const prev = fields.find((x) => x.name === f.name + PREV);
        rows.push({ t, f, next, prev, key: `${t.table}.${f.name}`, e: (t.embeddings || []).find((x) => x.field === f.name), en: next && (t.embeddings || []).find((x) => x.field === next.name) });
      }
      const embedded = new Set(fields.filter((f) => f.embed).flatMap((f) => [].concat(f.embed.from)));
      for (const f of fields) if (f.type === "text" && !embedded.has(f.name)) bare.push({ t, f });
    }
    const cols = [
      { key: "key", label: "Field", render: (r) => h("div", {}, h("b", { class: "mono" }, r.f.name), h("div", { class: "small muted" }, r.t.table)) },
      { key: "from", label: "Embeds", render: (r) => r.f.embed ? h("span", { class: "small dim" }, [].concat(r.f.embed.from).join(", ")) : h("span", { class: "small muted" }, "vectors given by the rows") },
      { key: "model", label: "Model", sort: (r) => r.f.embed?.model || "", render: (r) => {
        if (!r.f.embed) return h("span", { class: "muted small" }, "none");
        const tt = techniqueOf(r.f, models);
        return h("div", {}, h("div", { class: "mono small" }, r.f.embed.model), h("div", { class: "row nw", style: { marginTop: "3px" } }, techBadge(tt.technique, tt.model?.precision), h("span", { class: "small muted" }, `${r.f.dim || tt.model?.dim || "?"} dims${r.f.multi ? " a token" : ""}`)));
      } },
      { key: "search", label: "Searched with", sortable: false, render: (r) => { const tt = techniqueOf(r.f, models); return h("div", { class: "small" }, indexText(tt), h("span", { class: "muted" }, ` · ${tt.codes.toUpperCase().replace("RABITQ", "RaBitQ ")}${tt.keep_f32 ? " · f32 kept" : ""}`)); } },
      { key: "state", label: "Vectors", sortable: false, render: (r) => h("div", { class: "stack mm-state" }, vectorsState(r.e, fetches),
        r.next ? h("span", { class: "row nw small" }, pill("info", "changing"), h("span", { class: "mono" }, `→ ${r.next.embed?.model || "?"}`)) : null,
        r.prev ? h("span", { class: "row nw small" }, pill("info", "switched"), h("span", { class: "dim" }, `old vectors kept as ${r.prev.name}`)) : null) },
      { key: "act", label: "", sortable: false, render: (r) => {
        const b = h("button", { class: "btn sm" + (r.next || r.prev ? " primary" : ""), onclick: () => changeDrawer(r.t.table, r.f.name) }, icon(r.next || r.prev ? "rollout" : "sliders"), r.next ? "Continue" : r.prev ? "Finish" : "Change");
        return gate(b, "admin");
      } },
    ];
    const list = table2(cols, rows, empty("cpu", "No vector fields yet", "Add vectors to a text field below, or design a table with one in the Schema page."));
    const bareCard = bare.length ? card({
      title: "Text without vectors", sub: "Give one of these a vector field to search it by meaning, beside BM25",
      body: h("div", { class: "mm-bare" }, bare.map(({ t, f }) => gate(h("button", { class: "chip", onclick: () => addDrawer(t.table, f.name) }, icon("plus"), h("span", { class: "muted" }, t.table + "."), f.name), "admin"))),
    }) : null;
    return [card({ title: "Vector fields", sub: `${rows.length} in ${ts.length} ${ts.length === 1 ? "table" : "tables"}`, flush: true, body: list }), bareCard];
  }

  // ---------------------------------------------------------- models
  function modelsTab() {
    const order = Object.keys(TECHNIQUES);
    // Hosted models with no key and no field: named in a line, not a row each.
    const idle = (m) => m.technique === "hosted" && !m.ready && !m.fields?.length && !m.job;
    const rows = models.filter((m) => (m.technique !== "hash" || m.fields?.length) && !idle(m)).slice().sort((a, b) => order.indexOf(a.technique) - order.indexOf(b.technique) || (b.ready - a.ready) || a.name.localeCompare(b.name));
    const has = (n) => models.some((m) => m.name.toLowerCase() === n.toLowerCase() && (m.ready || m.job?.state === "running"));
    const cols = [
      { key: "name", label: "Model", render: (m) => h("div", {}, h("b", { class: "mono" }, m.name), m.made_of ? h("div", { class: "small muted" }, `a copy of ${m.made_of}`) : m.what ? h("div", { class: "small muted mm-what" }, m.what) : null) },
      { key: "technique", label: "Technique", sort: (m) => order.indexOf(m.technique), render: (m) => techBadge(m.technique, m.precision) },
      { key: "dim", label: "Dims", num: true },
      { key: "bytes", label: "Size", num: true, render: (m) => m.bytes ? fmt.bytes(m.bytes) : h("span", { class: "muted" }, "–") },
      { key: "fields", label: "Used by", sort: (m) => m.fields?.length || 0, render: (m) => m.fields?.length ? h("div", { class: "small" }, m.fields.map((f) => h("div", { class: "mono" }, `${f.table}.${f.field}`))) : h("span", { class: "muted small" }, "–") },
      { key: "ready", label: "Here", sort: (m) => m.ready ? 1 : 0, render: (m) => {
        if (m.job?.state === "running") return pill("pending", m.job.action === "convert" ? "converting" : "fetching");
        if (m.job?.state === "failed") return h("span", { title: m.job.error }, pill("bad", m.job.action + " failed"));
        if (m.ready) return pill("ok", m.hosted ? "key set" : "ready");
        return h("span", { title: m.error || "" }, pill(m.known && !/feature/.test(m.error || "") ? "pending" : "disabled", m.known && !/feature/.test(m.error || "") ? "not fetched" : m.hosted ? "no key" : "not built in"));
      } },
      { key: "act", label: "", sortable: false, render: (m) => {
        const out = [];
        if (m.known && !m.ready && !m.hosted && !/feature/.test(m.error || "") && m.job?.state !== "running") out.push(gate(h("button", { class: "btn sm", onclick: () => job("/v1/admin/models/fetch", { model: m.name }, `Fetching ${m.name}`) }, icon("download"), "Fetch"), "admin"));
        if (m.technique === "static" && m.ready && m.precision && m.precision !== "I8" && !m.name.endsWith("-int8") && !has(m.name + "-int8")) {
          out.push(gate(h("button", { class: "btn sm", title: "Write the table again in 8 bits: a model of its own, 4x smaller, near the same ranking", onclick: () => job("/v1/admin/models/convert", { model: m.name, to: "I8" }, `Converting ${m.name} to int8`) }, icon("box"), "Make int8"), "admin"));
        }
        return h("div", { class: "row nw" }, out);
      } },
    ];
    const explain = card({ title: "Techniques", sub: "How a model makes vectors decides its speed, its size and what it finds",
      body: h("div", { class: "mm-techs" }, order.filter((k) => !["hash", "unknown"].includes(k)).map((k) => h("div", { class: "mm-techcard", style: { "--c": TECHNIQUES[k].color } }, h("b", {}, TECHNIQUES[k].label), h("div", { class: "small dim" }, TECHNIQUES[k].what)))) });
    const more = models.filter(idle);
    const moreLine = more.length ? h("details", { class: "mm-more small dim" }, h("summary", {}, `${more.length} more hosted models, usable once their provider's key is in the server's environment`), h("div", { class: "mono small mm-what" }, more.map((m) => m.name).join(", "))) : null;
    return [
      card({ title: "Models", sub: dir ? `in ${dir}${fetches ? " · fields fetch the models they name" : ""}` : "", flush: true, body: table2(cols, rows, empty("cpu", "No models listed")), foot: moreLine }),
      explain,
    ];
  }

  async function job(path, body, what) {
    try { const j = await api(path, { body }); toast(`${what}: ${j.model}`, "info"); }
    catch (e) { toast(e.message, "bad"); return; }
    await load();
  }

  // ---------------------------------------------------------- change
  function changeDrawer(table, field) { openDrawer({ table, field, add: false }); }
  function addDrawer(table, text) { openDrawer({ table, text, add: true }); }

  function openDrawer(o) {
    const t = ts.find((x) => x.table === o.table);
    const s = t?.short || {};
    const fields = s.fields || [];
    const cur = o.add ? null : fields.find((f) => f.name === o.field);
    const taken = new Set(fields.map((f) => f.name));
    const newName = o.add ? (!taken.has("emb") ? "emb" : `${o.text}_emb`) : o.field + NEXT;
    const tt = cur ? techniqueOf(cur, models) : null;
    // The choice, starting from what the field is now.
    // Text models; hosted ones once their key is set.
    const pickable = models.filter((m) => (!["hash", "image-text", "sound-text", "unknown"].includes(m.technique) && (m.technique !== "hosted" || m.ready)) || m.name === cur?.embed?.model);
    const firstStatic = pickable.find((m) => m.technique === "static" && m.ready) || pickable.find((m) => m.technique === "static");
    const ch = {
      model: cur?.embed?.model && !fields.some((f) => f.name === o.field + NEXT) ? (firstStatic && firstStatic.name !== cur.embed.model ? firstStatic.name : cur.embed.model) : firstStatic?.name || pickable[0]?.name,
      from: cur?.embed ? [].concat(cur.embed.from) : [o.text],
      index: tt?.index || "hnsw", m: tt?.params?.m || 16, ef: tt?.params?.ef_construction || 200, nlist: tt?.params?.nlist || 256,
      codes: tt?.codes || "sq8", keep_f32: !!tt?.keep_f32, name: newName,
    };
    const d = drawer({ title: o.add ? `Add vectors to ${o.table}.${o.text}` : `Change ${o.table}.${o.field}`, sub: o.add ? "A vector field embedded from this text when rows are written" : `Now ${cur?.embed?.model || "vectors given by the rows"}${tt ? ` · ${indexText(tt)} · ${tt.codes}` : ""}`, wide: true, body: h("div"), onClose: () => { stop?.(); stop = null; load(true); } });
    let stop = null, lastSig = null, live = null;
    const paint = async (fresh) => {
      if (fresh) { invalidate(); try { [ts, models] = await Promise.all([tables(true), api("/v1/models").then((j) => j.models || [])]); } catch (e) { toast(e.message, "bad"); } }
      const t2 = ts.find((x) => x.table === o.table);
      const fs = t2?.short?.fields || [];
      const next = o.add ? null : fs.find((f) => f.name === o.field + NEXT);
      const prev = o.add ? null : fs.find((f) => f.name === o.field + PREV);
      const added = o.add ? fs.find((f) => f.name === ch.name && f.embed) : null;
      // Polled while vectors fill in: drawn again only when a step changes,
      // so a search typed into Compare stays; the counts change in place.
      const sig = JSON.stringify([fs.map((f) => f.name), (t2?.embeddings || []).map((e) => [e.field, e.loaded, !!e.error, e.backfill?.state])]);
      if (fresh && sig === lastSig && live) { live(t2); return; }
      lastSig = sig; live = null;
      clear(d.body, o.add ? (added ? addedStep(t2, added) : chooser(t2, fs)) : prev ? finishStep(t2, fs, prev) : next ? progressStep(t2, fs, next) : chooser(t2, fs));
    };

    // Step 1: the model and technique, and the ALTER that adds them.
    function chooser(t2, fs) {
      const out = h("div", { class: "stack" });
      const lab = (title, sub, el) => h("div", { class: "mm-lab" }, h("div", {}, h("b", { class: "small" }, title), sub ? h("div", { class: "small muted" }, sub) : null), el);
      const sqlBox = h("div");
      const model = () => models.find((m) => m.name === ch.model);
      const multi = () => model()?.technique === "late-interaction";
      const sync = () => clear(sqlBox, sqlBlock(addSql(o.table, ch, model())), notes());
      const notes = () => {
        const m = model(), out2 = [];
        if (!m) return null;
        if (!m.ready && m.known && !m.hosted) out2.push(fetches ? "The server fetches this model when the field is added, then fills in the vectors." : "Fetch this model first (Models tab): this server does not fetch models by itself.");
        if (!m.ready && m.hosted) out2.push(`This model needs its key in the server's environment: ${m.error || ""}`);
        if (ch.codes.startsWith("rabitq") && !fs.some((f) => (f.codes || "").startsWith("rabitq"))) out2.push("RaBitQ codes mark the database as needing a ChloeDB version that reads them: older versions refuse it.");
        if (cur && m.dim && cur.dim && m.dim !== cur.dim) out2.push(`The new vectors have ${m.dim} dimensions, the old ${cur.dim}: queries that pass their own vectors must change too. Text given for the field (similarity(${o.field}, 'words')) keeps working.`);
        return out2.length ? h("ul", { class: "small dim mm-notes" }, out2.map((x) => h("li", {}, x))) : null;
      };
      // The models, by technique.
      const groups = {};
      for (const m of pickable) (groups[m.technique] ||= []).push(m);
      const modelList = h("div", { class: "mm-pick" }, Object.keys(TECHNIQUES).filter((k) => groups[k]).map((k) => h("div", { class: "mm-group" },
        h("div", { class: "small muted mm-gh" }, TECHNIQUES[k].label),
        groups[k].map((m) => {
          const b = h("button", { type: "button", class: "mm-opt", "aria-pressed": String(m.name === ch.model), title: m.what || TECHNIQUES[k].what, onclick: () => { ch.model = m.name; if (multi()) { if (ch.codes.startsWith("rabitq")) ch.codes = "sq8"; } for (const x of modelList.querySelectorAll(".mm-opt")) x.setAttribute("aria-pressed", String(x === b)); techBox(); sync(); } },
            h("div", { class: "row nw between" }, h("b", { class: "mono small" }, m.name), m.name === cur?.embed?.model ? h("span", { class: "badge" }, "now") : null),
            h("div", { class: "small muted" }, [m.dim ? `${m.dim} dims` : null, m.precision && m.precision !== "F32" ? (m.precision === "I8" ? "int8" : m.precision) : null, m.bytes ? fmt.bytes(m.bytes) : null, m.ready ? null : m.known && !m.hosted && !/feature/.test(m.error || "") ? "not fetched" : m.hosted ? "needs its key" : "not built in"].filter(Boolean).join(" · ")));
          if (!m.ready && !m.known && !m.hosted) b.disabled = true;
          if (/feature/.test(m.error || "")) b.disabled = true;
          return b;
        }))));
      // The fields it embeds.
      const srcs = fs.filter((f) => f.type === "text" || f.type === "keyword");
      const fromBox = h("div", { class: "row" }, srcs.map((f) => h("button", { type: "button", class: "chip", "aria-pressed": String(ch.from.includes(f.name)), onclick: () => { ch.from = ch.from.includes(f.name) ? ch.from.filter((x) => x !== f.name) : [...ch.from, f.name]; if (!ch.from.length) ch.from = [f.name]; for (const c of fromBox.children) c.setAttribute("aria-pressed", String(ch.from.includes(c.textContent))); sync(); } }, f.name)));
      // How it is searched and stored.
      const tbox = h("div", { class: "stack" });
      function techBox() {
        const params = h("div", { class: "row" });
        const num = (label, k, min, max) => h("label", { class: "row nw small" }, h("span", { class: "dim" }, label), h("input", { class: "input sm", type: "number", min, max, value: ch[k], style: { width: "84px" }, oninput: (e) => { ch[k] = Math.max(min, Math.min(max, +e.target.value || min)); sync(); } }));
        const drawParams = () => clear(params, ch.index === "hnsw" ? [num("m", "m", 4, 96), num("ef construction", "ef", 16, 2000)] : ch.index === "ivf" ? [num("clusters", "nlist", 2, 65536)] : h("span", { class: "small muted" }, "every row is scored: exact"));
        const codes = multi() ? CODES.filter(([c]) => !c.startsWith("rabitq")) : CODES;
        clear(tbox,
          lab("Search", "How the nearest rows are found", h("div", { class: "stack", style: { gap: "6px" } }, seg(INDEXES, ch.index, (v) => { ch.index = v; drawParams(); sync(); }), params)),
          lab("Codes", "Compact copies of the vectors that the search scores first; the float vectors rescore the best", h("div", { class: "stack", style: { gap: "6px" } }, seg(codes.map(([v, l, w]) => [v, l, w]), ch.codes, (v) => { ch.codes = v; hint.textContent = CODES.find((c) => c[0] === v)?.[2] || ""; sync(); }), hint)),
          multi() ? lab("Floats", "Keep every token's float vector to score the best rows exactly (more disk)", toggle(ch.keep_f32, (v) => { ch.keep_f32 = v; sync(); }, "keep float32 vectors")) : null);
        drawParams();
      }
      const hint = h("div", { class: "small muted mm-hint" }, CODES.find((c) => c[0] === ch.codes)?.[2] || "");
      techBox();
      const nameIn = h("input", { class: "input sm mono", value: ch.name, style: { width: "220px" }, oninput: (e) => { ch.name = e.target.value.trim(); sync(); } });
      const run = gate(h("button", { class: "btn primary", onclick: async () => {
        const m = model();
        if (!m) return;
        if (!ch.name || fs.some((f) => f.name === ch.name)) { toast(`${ch.name} is a field of ${o.table} already`, "bad"); return; }
        const sql = addSql(o.table, ch, m);
        run.disabled = true;
        try { await api("/v1/sql", { body: { sql } }); toast(o.add ? `Added ${ch.name}` : `Added ${ch.name}: its vectors are being filled in`, "ok"); }
        catch (e) { toast(e.message, "bad"); run.disabled = false; return; }
        await paint(true);
        watch();
      } }, icon("plus"), o.add ? "Add the field" : "Add the new field beside the old"), "admin");
      out.append(
        steps(o.add ? 0 : 1, !o.add),
        lab("Model", "The model that makes the vectors. Its technique decides speed, size and what a search finds.", modelList),
        lab("Of the fields", "The text the model embeds, joined", fromBox),
        tbox,
        lab(o.add ? "Field name" : "New field", o.add ? "" : `Added beside ${o.field}; it takes ${o.field}'s name when you switch`, o.add ? nameIn : h("span", { class: "mono" }, ch.name)),
        lab("The change", "Run as SQL, as you could in the workbench", sqlBox),
        h("div", { class: "row" }, run));
      sync();
      return out;
    }

    // Steps 2 and 3: filling in, then comparing and switching.
    function progressStep(t2, fs, next) {
      const e = (t2.embeddings || []).find((x) => x.field === next.name);
      const out = h("div", { class: "stack" });
      const b = e?.backfill;
      const filling = !e || !e.loaded || b?.state === "running";
      const state = h("div", { class: "row" });
      const fill = (e2) => clear(state, vectorsState(e2, fetches), h("span", { class: "small dim" }, e2?.error ? e2.error : filling ? "The rows already there are given their new vectors in the background; searches use the old field meanwhile." : "Every row has its new vectors."));
      fill(e);
      live = (t3) => fill((t3?.embeddings || []).find((x) => x.field === next.name));
      const abandon = gate(h("button", { class: "btn danger", onclick: async () => {
        const ok = await confirm({ title: `Drop ${next.name}?`, body: `The new vectors go; ${o.field} stays as it is.`, ok: "Drop it", danger: true });
        if (!ok) return;
        try { await api("/v1/sql", { body: { sql: `ALTER TABLE ${quoteName(o.table)} DROP COLUMN ${quoteName(next.name)}` } }); toast(`Dropped ${next.name}`, "ok"); }
        catch (err) { toast(err.message, "bad"); return; }
        await paint(true);
      } }, icon("x"), "Cancel the change"), "admin");
      const switchBtn = gate(h("button", { class: "btn primary", disabled: filling, title: filling ? "after the vectors are filled in" : "", onclick: async () => {
        const sql = `ALTER TABLE ${quoteName(o.table)} RENAME COLUMN ${quoteName(o.field)} TO ${quoteName(o.field + PREV)}, RENAME COLUMN ${quoteName(next.name)} TO ${quoteName(o.field)}`;
        const ok = await confirm({ title: `Switch ${o.field} to ${next.embed?.model}?`, body: h("div", { class: "stack" }, h("p", { class: "dim", style: { margin: 0 } }, `Queries that name ${o.field} use the new vectors from now on. The old ones stay as ${o.field + PREV} until you remove them, so the switch can be undone.`), sqlBlock(sql)), ok: "Switch" });
        if (!ok) return;
        try { await api("/v1/sql", { body: { sql } }); toast(`${o.field} is ${next.embed?.model} now`, "ok"); }
        catch (err) { toast(err.message, "bad"); return; }
        await paint(true);
      } }, icon("rollout"), "Switch to the new field"), "admin");
      out.append(steps(filling ? 2 : 3, true),
        card({ title: `${next.name}`, sub: `${next.embed?.model} · ${indexText(techniqueOf(next, models))} · ${next.codes || "sq8"}`, body: state }),
        compare(t2, fs, o.field, next.name, filling),
        h("div", { class: "row" }, switchBtn, abandon));
      if (filling) watch();
      return out;
    }

    // Step 4: remove the old vectors, or undo.
    function finishStep(t2, fs, prev) {
      const out = h("div", { class: "stack" });
      const undo = gate(h("button", { class: "btn", onclick: async () => {
        const sql = `ALTER TABLE ${quoteName(o.table)} RENAME COLUMN ${quoteName(o.field)} TO ${quoteName(o.field + NEXT)}, RENAME COLUMN ${quoteName(prev.name)} TO ${quoteName(o.field)}`;
        const ok = await confirm({ title: "Undo the switch?", body: h("div", { class: "stack" }, h("p", { class: "dim", style: { margin: 0 } }, `${o.field} gets its old vectors back (${prev.embed?.model || "?"}); the new ones wait as ${o.field + NEXT}.`), sqlBlock(sql)), ok: "Undo" });
        if (!ok) return;
        try { await api("/v1/sql", { body: { sql } }); toast("Switched back", "ok"); } catch (err) { toast(err.message, "bad"); return; }
        await paint(true);
      } }, icon("undo"), "Undo the switch"), "admin");
      const drop = gate(h("button", { class: "btn danger", onclick: async () => {
        const sql = `ALTER TABLE ${quoteName(o.table)} DROP COLUMN ${quoteName(prev.name)}`;
        const ok = await confirm({ title: `Remove ${prev.name}?`, body: h("div", { class: "stack" }, h("p", { class: "dim", style: { margin: 0 } }, "The old vectors are deleted, and the switch can no longer be undone. A coordinator keeps the column (it refuses DROP COLUMN): its vectors stay until the shards' tables are changed one by one."), sqlBlock(sql)), ok: "Remove", danger: true, typed: prev.name });
        if (!ok) return;
        try { await api("/v1/sql", { body: { sql } }); toast(`Removed ${prev.name}`, "ok"); } catch (err) { toast(err.message, "bad"); return; }
        await paint(true);
      } }, icon("x"), "Remove the old vectors"), "admin");
      const cur2 = fs.find((f) => f.name === o.field);
      out.append(steps(4, true),
        h("p", { class: "dim", style: { margin: 0 } }, `${o.field} is ${cur2?.embed?.model || "?"} now. The old vectors (${prev.embed?.model || "?"}) are kept as `, h("span", { class: "mono" }, prev.name), "."),
        compare(t2, fs, o.field, prev.name, false, ["now", "before"]),
        h("div", { class: "row" }, drop, undo));
      return out;
    }

    function addedStep(t2, added) {
      const e = (t2.embeddings || []).find((x) => x.field === added.name);
      const filling = !e || !e.loaded || e.backfill?.state === "running";
      if (filling) watch();
      return h("div", { class: "stack" },
        h("div", { class: "row" }, vectorsState(e, fetches), h("span", { class: "small dim" }, filling ? "The rows already there are given vectors in the background." : "Every row has its vectors.")),
        compare(t2, t2.short.fields, null, added.name, filling, ["", added.name]),
        h("div", { class: "row" }, h("button", { class: "btn", onclick: () => d.close() }, "Done")));
    }

    // The same text searched on two fields, side by side.
    function compare(t2, fs, a, b, filling, names) {
      const key = t2.short?.key || t2.primary_key;
      const text = [].concat(fs.find((f) => f.name === b)?.embed?.from || [])[0] || fs.find((f) => f.type === "text")?.name;
      const q = h("input", { class: "input", placeholder: "Words to search for, as a user would", value: store.get(`models.q.${o.table}`, ""), style: { flex: "1 1 260px" } });
      const outA = h("div"), outB = h("div");
      const cols = a ? h("div", { class: "grid g2" }, h("div", {}, h("div", { class: "small muted mm-colh" }, (names?.[0] || "now") + ": ", h("span", { class: "mono" }, a)), outA), h("div", {}, h("div", { class: "small muted mm-colh" }, (names?.[1] || "new") + ": ", h("span", { class: "mono" }, b)), outB)) : outB;
      const overlap = h("div", { class: "small dim" });
      const order = (f) => fs.find((x) => x.name === f)?.multi ? "maxsim" : "similarity";
      const one = async (f, el) => {
        const sql = `SELECT ${quoteName(key)}${text ? `, ${quoteName(text)}` : ""} FROM ${quoteName(o.table)} ORDER BY ${order(f)}(${quoteName(f)}, :q) DESC LIMIT 10`;
        const t0 = performance.now();
        try {
          const j = await api("/v1/sql", { body: { sql, params: { q: q.value } } });
          const ms = performance.now() - t0;
          clear(el, h("ol", { class: "mm-hits" }, j.rows.map((r) => h("li", {}, h("span", { class: "mono small" }, String(r[0])), r[1] != null ? h("span", { class: "small dim" }, " " + String(r[1]).slice(0, 120)) : null))), h("div", { class: "small muted" }, `${fmt.ms(ms)} in the browser`));
          return j.rows.map((r) => String(r[0]));
        } catch (e) { clear(el, h("div", { class: "error small" }, e.message)); return null; }
      };
      const go = async () => {
        if (!q.value.trim()) return;
        store.set(`models.q.${o.table}`, q.value);
        const [x, y] = await Promise.all([a ? one(a, outA) : null, one(b, outB)]);
        if (x && y) { const same = y.filter((k) => x.includes(k)).length; overlap.textContent = `${same} of the top ${Math.max(x.length, y.length)} rows are in both.`; }
      };
      q.onkeydown = (e) => { if (e.key === "Enter") go(); };
      // The last words searched, searched again on both.
      if (q.value.trim()) setTimeout(go, 0);
      return card({ title: "Compare", sub: filling ? "Rows not yet filled in are missing from the new results" : "The same words searched on each field",
        body: h("div", { class: "stack" }, h("div", { class: "row" }, q, h("button", { class: "btn", onclick: go }, icon("search"), "Search both")), overlap, cols) });
    }

    function steps(at, change) {
      const names = change ? ["Choose", "Fill in", "Compare and switch", "Remove the old"] : ["Choose"];
      return h("ol", { class: "mm-steps" }, names.map((n, i) => h("li", { class: i + 1 < at ? "done" : i + 1 === at ? "now" : "" }, h("span", { class: "mm-n" }, i + 1 < at ? "✓" : String(i + 1)), n)));
    }

    // While vectors fill in: read the tables again every few seconds.
    // While vectors fill in (or a model is fetched): read the tables
    // again every few seconds, for at most half an hour.
    function watch() {
      if (stop) return;
      const until = Date.now() + 30 * 60e3;
      stop = every(3000, async () => {
        await paint(true);
        const t2 = ts.find((x) => x.table === o.table);
        const busy = (t2?.embeddings || []).some((e) => !e.loaded || e.backfill?.state === "running");
        if (!busy || Date.now() > until) { stop?.(); stop = null; }
      }, false);
    }
    paint(false);
  }

  load();
  const off = every(5000, async () => { if (models.some((m) => m.job?.state === "running")) await load(); }, false);
  return () => { alive = false; off(); };
}

/** The ALTER TABLE that adds a field of choice `ch` with model `m`. */
export function addSql(table, ch, m) {
  const multi = m?.technique === "late-interaction";
  const dim = m?.dim ? `(${m.dim})` : "";
  const opts = [`embed = '${ch.from.join(", ").replace(/'/g, "''")}'`, `model = '${String(ch.model).replace(/'/g, "''")}'`];
  if (ch.index !== "hnsw" || ch.m !== 16 || ch.ef !== 200) opts.push(`index = '${ch.index}'`);
  if (ch.index === "hnsw" && (ch.m !== 16 || ch.ef !== 200)) opts.push(`m = ${ch.m}`, `ef_construction = ${ch.ef}`);
  if (ch.index === "ivf") opts.push(`nlist = ${ch.nlist}`);
  if (ch.codes !== "sq8") opts.push(`codes = '${ch.codes}'`);
  if (multi && ch.keep_f32) opts.push("keep_f32 = true");
  return `ALTER TABLE ${quoteName(table)} ADD COLUMN ${quoteName(ch.name)} VECTOR${dim}${multi ? "[]" : ""} OPTIONS(${opts.join(", ")})`;
}

/** The sortable table of core.js, with an empty state. */
function table2(cols, rows, emptyEl) { return coreTable(cols, { rows, empty: emptyEl }); }

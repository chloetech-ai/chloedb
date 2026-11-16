// The two workflows of the Understanding page (views/understand.js).
//
// Enrich rows: fields that models make of each row's text when it is
// written, chosen as steps (a language, main phrases, near duplicates,
// the names it mentions, a class, a language model's text, a vector for
// meaning, a caption or transcript of a file), tried on sample rows
// (/v1/derive/preview, llm(), embed()), added with ALTER TABLE ... ADD
// COLUMN, and the rows written before filled in: an UPDATE that writes
// each row's source again, a page of keys at a time, which makes the
// derived values as any write does (the server fills in vectors itself).
//
// Enrich queries: what a search says beyond its words, read in steps
// (its spelling, the values it names, its class, its language, the names
// it mentions, what the caller knows of the person) and added to the
// request as filters and options; the request is shown as SQL, run beside
// plain BM25, and saved as a search whose filters are parameters, so the
// code that calls it passes what it read of each query.

import { h, icon, clear, api, card, empty, failure, toast, copy, gate, can, sqlBlock, seg, toggle, confirm, fmt } from "../core.js";
import { quoteName } from "../sqledit.js";
import { invalidate, tables } from "../catalog.js";
import { saveSearch, useFromCode } from "./saved.js";
import { isCoordinator } from "../data.js";

const lit = (v) => `'${String(v).replace(/'/g, "''")}'`;
const litv = (v) => (typeof v === "number" ? String(v) : lit(v));
const clip = (s, n) => { s = String(s ?? ""); return s.length > n ? s.slice(0, n - 1) + "…" : s; };
const fieldsOf = (t) => t?.short?.fields || [];
const isStored = (f) => f.stored ?? !(f.from && f.from.length);
const ident = (s) => String(s || "").toLowerCase().replace(/[^a-z0-9_]+/g, "_").replace(/^_+|_+$/g, "").replace(/^(\d)/, "_$1") || "field";
const plural = (n, one, many = one + "s") => `${fmt.n ? fmt.n(n) : n} ${n === 1 ? one : many}`;

/** A field a model may read: a stored TEXT or KEYWORD of one value, not made of others, or a MEDIA field. */
export function sourceFields(t) {
  return fieldsOf(t).filter((f) => (f.type === "media") || ((f.type === "text" || f.type === "keyword") && !f.multi && isStored(f) && !f.derive && !(f.from && f.from.length)));
}

/** The kinds of step, by group: what each makes, its SQL type and model. */
const KINDS = [
  ["Built in: no model files, microseconds a row", [
    { id: "lang", label: "Language", what: "the text's language as a code: en, de, zh", type: "KEYWORD", model: "builtin:lang", name: "lang" },
    { id: "keyphrases", label: "Main phrases", what: "its five main phrases, for facets and tags", type: "KEYWORD[]", model: "builtin:keyphrases", name: "phrases" },
    { id: "minhash", label: "Near duplicates", what: "bands that two near copies share", type: "KEYWORD[]", model: "builtin:minhash", name: "dup" },
  ]],
  ["From your tables", [
    { id: "dict", label: "Names it mentions", what: "the values of another table's KEYWORD field it names in full: brands, places, products", type: "KEYWORD[]", name: "mentions" },
    { id: "class", label: "Class", what: "a classifier's label of it (train one under Configure)", type: "KEYWORD", name: "class" },
  ]],
  ["Models", [
    { id: "llm", label: "Text by a language model", what: "a summary, a category, the product a review names: a prompt says what", type: "TEXT", name: "summary" },
    { id: "embed", label: "Vector for meaning", what: "search by meaning and similar rows, from an embedding model", type: "VECTOR", name: "emb" },
    { id: "caption", label: "Caption of an image", what: "BLIP's caption of each image", type: "TEXT", model: "blip-image-captioning-base", media: "image", name: "caption" },
    { id: "transcript", label: "Transcript of a recording", what: "Whisper's transcript of each recording", type: "TEXT", model: "whisper-small", media: "audio", name: "transcript" },
  ]],
];
const KIND = Object.fromEntries(KINDS.flatMap(([, ks]) => ks.map((k) => [k.id, k])));
const LLMS = ["anthropic/claude-haiku-5-5", "anthropic/claude-sonnet-5-5", "openai/gpt-5-mini"];
const PROMPTS = {
  summary: "Summarize this text in one sentence.",
  category: "Name the category of this text in one or two words. Answer with the category only.",
  product: "Name the product this text is about, in at most five words. Answer with the name only.",
};

/** The model a step names. */
function modelOf(s) {
  if (s.kind === "dict") return s.dict ? `dict:${s.dict}` : null;
  if (s.kind === "class") return s.classifier ? `classifier:${s.classifier}` : null;
  if (s.kind === "llm" || s.kind === "embed") return s.model || null;
  return KIND[s.kind].model;
}

/** The column a step adds, as ADD COLUMN takes it. */
function columnSql(s, src, models) {
  const m = modelOf(s), k = KIND[s.kind], n = quoteName(s.name);
  if (s.kind === "embed") {
    const dim = (models.find((x) => x.name === m)?.dim) || (/^hash:(\d+)$/.exec(m || "")?.[1]);
    return `${n} VECTOR${dim ? `(${dim})` : ""} OPTIONS(embed = ${lit(src)}, model = ${lit(m)})`;
  }
  if (s.kind === "llm") return `${n} TEXT OPTIONS(derive = ${lit(src)}, model = ${lit(m)}, prompt = ${lit(s.prompt || "")})`;
  return `${n} ${k.type} OPTIONS(derive = ${lit(src)}, model = ${lit(m)})`;
}

/** Whether a step is filled in by the writer on its own (a vector) or needs rows written again. */
const fillsItself = (s) => s.kind === "embed";

// ======================================================================
// Enrich rows
// ======================================================================

/**
 * The content workflow. env: {body, st, save, table: () => the table's
 * catalog entry, classifiers: () => [...], redraw()}.
 */
export function enrichRows(env) {
  const { body, st, save } = env;
  const T = env.table(), name = T.table, key = T.short?.key;
  const srcs = sourceFields(T);
  if (!srcs.length) { clear(body, empty("sparkle", "Nothing for a model to read", `${name} has no stored TEXT, KEYWORD or MEDIA field of one value.`)); return; }
  const w = st.rows && st.rows.table === name ? st.rows : (st.rows = { table: name, src: (srcs.find((f) => f.type === "text" && /desc|body|text|content|abstract|summary|review/i.test(f.name)) || srcs.find((f) => f.name === st.field) || srcs.find((f) => f.type === "text") || srcs[0]).name, steps: [], find: "" });
  if (!srcs.some((f) => f.name === w.src)) w.src = srcs[0].name;
  const srcField = () => srcs.find((f) => f.name === w.src);
  const isMedia = () => srcField()?.type === "media";
  let models = [], samples = null;
  const names = new Set(fieldsOf(T).map((f) => f.name.toLowerCase()));

  const s1 = h("div", { class: "stack", style: { gap: "8px" } });
  const s2 = h("div", { class: "stack", style: { gap: "8px" } });
  const s3 = h("div", { class: "stack", style: { gap: "8px" } });
  const s4 = h("div", { class: "stack", style: { gap: "8px" } });
  const made = h("div");
  const step = (n, title, what, box) => h("li", {}, h("span", { class: "start-n" }, String(n)), h("div", { class: "grow stack", style: { gap: "6px", minWidth: 0 } }, h("b", {}, title), what ? h("div", { class: "small dim" }, what) : null, box));
  clear(body,
    h("ol", { class: "start-steps" },
      step(1, "What the models read", "A field of each row: its text, or the file a MEDIA field names.", s1),
      step(2, "What to make of it", "Each step is a new field, made when a row is written and kept with it: filters, facets and searches read it as any field.", s2),
      step(3, "Try it on sample rows", "What each step makes of a few of the table's rows, before anything is added.", s3),
      step(4, "Add the fields", "One ALTER TABLE. Rows written from then on get the values; the rows already there are filled in next.", s4)),
    made);

  api("/v1/models").then((j) => { models = j.models || []; drawSteps(); drawAdd(); }).catch(() => {});
  drawSource(); drawSteps(); drawTry(); drawAdd(); drawMade();

  function drawSource() {
    const sel = h("select", { class: "select", onchange: () => { w.src = sel.value; w.steps = w.steps.filter((s) => kindFits(KIND[s.kind])); samples = null; save(); drawSource(); drawSteps(); drawTry(); drawAdd(); } },
      srcs.map((f) => h("option", { value: f.name, selected: f.name === w.src }, `${f.name} (${f.type}${f.kind ? " of " + f.kind : ""})`)));
    const find = h("input", { class: "input", placeholder: "Sample: rows with these words (or the first rows)", value: w.find || "", style: { flex: "1 1 220px", minWidth: 0 } });
    const go = () => { w.find = find.value.trim(); samples = null; save(); drawTry(); };
    find.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    clear(s1, h("div", { class: "row", style: { flexWrap: "wrap", gap: "8px" } }, h("span", { class: "small dim" }, "Field"), sel, isMedia() ? null : find, isMedia() ? null : h("button", { class: "btn sm", onclick: go }, "Sample")));
  }

  function kindFits(k) {
    const f = srcField();
    if (!f) return false;
    if (f.type === "media") return (k.media && k.media === (f.kind || "image")) || (k.id === "embed");
    return !k.media;
  }

  function freeName(base) {
    let n = ident(base), i = 2;
    const taken = (x) => names.has(x) || w.steps.some((s) => s.name.toLowerCase() === x);
    while (taken(n)) n = `${ident(base)}_${i++}`;
    return n;
  }

  function addStep(k) {
    const s = { kind: k.id, name: freeName(k.name) };
    if (k.id === "llm") { s.model = LLMS[0]; s.prompt = PROMPTS.summary; }
    if (k.id === "embed") {
      const ok = models.filter((m) => fitsEmbed(m));
      s.model = (ok.find((m) => m.ready && !m.hosted && !m.name.includes("/")) || ok.find((m) => /potion-retrieval-32M-int8/.test(m.name)) || ok[0])?.name || "potion-retrieval-32M-int8";
    }
    if (k.id === "class") s.classifier = env.classifiers()[0]?.name || "";
    w.steps.push(s); save(); drawSteps(); drawTry(); drawAdd();
  }

  function fitsEmbed(m) {
    const media = isMedia() ? (srcField().kind || "image") : null;
    if (media === "image") return m.technique === "image-text";
    if (media === "audio") return m.technique === "sound-text";
    return ["static", "transformer", "hosted"].includes(m.technique);
  }

  function drawSteps() {
    const picker = h("div", { class: "mm-pick" }, KINDS.map(([g, ks]) => {
      const fit = ks.filter(kindFits);
      if (!fit.length) return null;
      return h("div", { class: "mm-group" }, h("div", { class: "mm-gh small dim" }, g),
        fit.map((k) => h("button", { type: "button", class: "mm-opt", onclick: () => addStep(k), title: `Adds a ${k.type} field` }, h("b", {}, k.label), h("span", { class: "small dim" }, k.what))));
    }));
    const list = w.steps.map((s, i) => stepCard(s, i));
    clear(s2, ...list, w.steps.length ? h("details", { class: "mm-more" }, h("summary", { class: "small" }, "Add another step"), picker) : picker);
  }

  function stepCard(s, i) {
    const k = KIND[s.kind];
    const nameIn = h("input", { class: "input sm mono", value: s.name, style: { width: "150px" }, spellcheck: "false", oninput: () => { s.name = ident(nameIn.value); save(); drawAdd(); }, onchange: () => { nameIn.value = s.name; } });
    const opts = h("div", { class: "row", style: { flexWrap: "wrap", gap: "8px" } });
    if (s.kind === "dict") {
      const opts2 = [];
      Promise.resolve(tables()).then((ts) => {
        for (const t of ts) for (const f of fieldsOf(t)) if (f.type === "keyword" && !f.derive && !t.table.startsWith("_")) opts2.push(`${t.table}.${f.name}`);
        const sel = h("select", { class: "select sm", onchange: () => { s.dict = sel.value; save(); drawAdd(); drawTry(); } }, h("option", { value: "" }, "a table's KEYWORD field…"), opts2.map((o) => h("option", { value: o, selected: o === s.dict }, o)));
        clear(opts, h("span", { class: "small dim" }, "values of"), sel,
          isCoordinator() ? h("div", { class: "small", style: { color: "var(--serious)", flexBasis: "100%" } }, icon("warn"), " On a cluster each shard reads only the names its own part of that table holds, so a row misses the names the other shards hold (the samples here read them all). Until that changes, write the values with the rows.") : null);
      }).catch((e) => clear(opts, failure(e, "The tables")));
    } else if (s.kind === "class") {
      const cs = env.classifiers();
      if (!cs.length) opts.append(h("span", { class: "small dim" }, "No classifier yet: train one under Configure, from a column of labels and a vector field."));
      else opts.append(h("span", { class: "small dim" }, "classifier"), h("select", { class: "select sm", onchange: (e) => { s.classifier = e.target.value; save(); drawAdd(); drawTry(); } }, cs.map((c) => h("option", { value: c.name, selected: c.name === s.classifier }, `${c.name} (${c.labels} labels of ${c.labels_from})`))));
    } else if (s.kind === "llm") {
      const m = h("input", { class: "input sm mono", value: s.model, list: "enrich-llms", style: { width: "230px" }, oninput: () => { s.model = m.value.trim(); save(); drawAdd(); } });
      const p = h("textarea", { class: "input", rows: 2, style: { width: "100%" }, oninput: () => { s.prompt = p.value; save(); drawAdd(); } }, s.prompt || "");
      opts.append(h("span", { class: "small dim" }, "model"), m, h("datalist", { id: "enrich-llms" }, LLMS.map((x) => h("option", { value: x }))),
        ...Object.entries(PROMPTS).map(([n, t]) => h("button", { class: "btn xs ghost", onclick: () => { s.prompt = t; p.value = t; if (/^summary(_\d+)?$/.test(s.name) && n !== "summary") { s.name = freeName(n); } save(); drawSteps(); drawAdd(); } }, n)),
        h("div", { style: { flexBasis: "100%" } }, p),
        h("div", { class: "small dim" }, "Each row written is one request to the model (eight at once). Its key is the server's (ANTHROPIC_API_KEY, OPENAI_API_KEY), or a chat server of your own named by an endpoint.json in the models folder."));
    } else if (s.kind === "embed") {
      const ok = models.filter(fitsEmbed);
      const sel = h("select", { class: "select sm", onchange: () => { s.model = sel.value; save(); drawAdd(); drawTry(); } },
        (ok.some((m) => m.name === s.model) ? ok : [{ name: s.model }, ...ok]).map((m) => h("option", { value: m.name, selected: m.name === s.model }, `${m.name}${m.dim ? ` · ${m.dim} numbers` : ""}${m.ready ? " · ready" : m.hosted ? " · an API's" : " · fetched when added"}`)));
      const m = ok.find((x) => x.name === s.model);
      opts.append(h("span", { class: "small dim" }, "model"), sel, m?.what ? h("span", { class: "small dim" }, m.what) : null);
    } else {
      opts.append(h("span", { class: "small dim mono" }, modelOf(s)));
    }
    const rm = h("button", { class: "btn xs ghost", title: "Remove this step", onclick: () => { w.steps.splice(i, 1); save(); drawSteps(); drawTry(); drawAdd(); } }, icon("x"), "Remove");
    return h("div", { class: "card ex-step" },
      h("div", { class: "row", style: { flexWrap: "wrap", gap: "8px" } }, h("b", {}, k.label), h("span", { class: "chip mono" }, k.type), h("span", { class: "small dim" }, "into"), nameIn, h("div", { class: "grow" }), rm),
      opts);
  }

  // ---- 3. try it
  async function sampleRows() {
    if (samples) return samples;
    const K = quoteName(key || w.src), S = quoteName(w.src), T0 = quoteName(name);
    const text = fieldsOf(T).find((f) => f.name === st.field && f.type === "text") ? quoteName(st.field) : null;
    const sql = w.find && text ? `SELECT ${K}, ${S} FROM ${T0} WHERE MATCH(${text}, :q) ORDER BY bm25(${text}, :q) DESC LIMIT 5` : `SELECT ${K}, ${S} FROM ${T0} WHERE ${S} IS NOT NULL LIMIT 5`;
    const j = await api("/v1/query", { body: { sql, params: w.find ? { q: w.find } : {} } });
    samples = (j.rows || []).map((r) => ({ key: r[0], text: r[1] }));
    return samples;
  }

  function drawTry() {
    if (isMedia()) { clear(s3, h("div", { class: "small dim" }, "A caption, transcript or vector of a file is made when the row is written, by a model the server runs: add the field, then look at a row under Inspect a row.")); return; }
    if (!w.steps.length) { clear(s3, h("div", { class: "small dim" }, "Add a step first.")); return; }
    const out = h("div", { class: "tbl-wrap" }, h("div", { class: "small dim" }, "…"));
    const asks = w.steps.some((s) => s.kind === "llm");
    const ask = asks ? h("button", { class: "btn sm", onclick: () => run(true) }, icon("sparkle"), "Ask the language model too") : null;
    clear(s3, out, asks ? h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } }, ask, h("span", { class: "small dim" }, "One request a sample row, with the server's key.")) : null);
    run(false);

    async function run(withModel) {
      let rows;
      try { rows = await sampleRows(); } catch (e) { clear(out, failure(e, "Sample rows")); return; }
      if (!rows.length) { clear(out, h("div", { class: "small dim" }, w.find ? "No row has these words." : `${name} has no row with ${w.src} yet.`)); return; }
      const cells = rows.map(() => w.steps.map(() => h("span", { class: "small dim" }, "…")));
      clear(out, h("table", { class: "t compact ex-try" },
        h("thead", {}, h("tr", {}, h("th", {}, w.src), ...w.steps.map((s) => h("th", { class: "mono" }, s.name)))),
        h("tbody", {}, rows.map((r, i) => h("tr", {}, h("td", { style: { maxWidth: "360px" } }, h("div", { class: "small" }, clip(r.text, 220))), ...cells[i].map((c) => h("td", { style: { maxWidth: "260px" } }, c)))))));
      w.steps.forEach((s, j) => {
        const m = modelOf(s);
        const put = (i, el) => clear(cells[i][j].parentElement || cells[i][j], el);
        if (!m) { rows.forEach((_, i) => put(i, h("span", { class: "small dim" }, "pick its options"))); return; }
        if (s.kind === "llm") {
          if (!withModel) { rows.forEach((_, i) => put(i, h("span", { class: "small dim" }, "not asked yet"))); return; }
          const K = quoteName(key);
          const sql = `SELECT ${K}, llm(${lit(m)}, ${lit(s.prompt || "")}, ${quoteName(w.src)}) FROM ${quoteName(name)} WHERE ${K} IN (${rows.map((r) => litv(r.key)).join(", ")})`;
          api("/v1/query", { body: { sql } }).then((a) => {
            const by = new Map((a.rows || []).map((x) => [String(x[0]), x[1]]));
            rows.forEach((r, i) => put(i, h("div", { class: "small" }, clip(by.get(String(r.key)) ?? "", 240))));
          }).catch((e) => rows.forEach((_, i) => put(i, i === 0 ? failure(e, "The model") : h("span"))));
          return;
        }
        if (s.kind === "embed") {
          api("/v1/query", { body: { sql: `SELECT embed(${lit(m)}, :t) AS v FROM ${quoteName(name)} LIMIT 1`, params: { t: String(rows[0].text) } } }).then((a) => {
            const v = a.rows?.[0]?.[0];
            rows.forEach((_, i) => put(i, i === 0 ? h("span", { class: "small" }, Array.isArray(v) ? `${v.length} numbers (${v.slice(0, 3).map((x) => x.toFixed(3)).join(", ")}, …)` : "a vector") : h("span", { class: "small dim" }, "a vector")));
          }).catch((e) => rows.forEach((_, i) => put(i, i === 0 ? h("div", { class: "small dim" }, `Made once the model is open: ${e.message}`) : h("span"))));
          return;
        }
        rows.forEach((r, i) => {
          const b = { model: m, text: String(r.text ?? ""), table: name, field: w.src, many: KIND[s.kind].type.endsWith("[]") };
          api("/v1/derive/preview", { body: b }).then((a) => {
            const vs = s.kind === "minhash" ? a.values.slice(0, 2) : a.values;
            put(i, vs.length ? h("div", { class: "row", style: { flexWrap: "wrap", gap: "4px" } }, vs.map((x) => h("span", { class: "chip" + (s.kind === "minhash" ? " mono" : "") }, clip(x, 40))), s.kind === "minhash" && a.values.length > 2 ? h("span", { class: "small dim" }, `+${a.values.length - 2}`) : null) : h("span", { class: "small dim" }, "(none)"));
          }).catch((e) => put(i, h("span", { class: "small dim", title: e.message }, clip(e.message, 80))));
        });
      });
    }
  }

  // ---- 4. add
  function drawAdd() {
    const ready = w.steps.filter((s) => modelOf(s) && s.name && (s.kind !== "llm" || (s.prompt || "").trim()));
    if (!ready.length) { clear(s4, h("div", { class: "small dim" }, "The SQL appears here once a step has its options.")); return; }
    const sql = `ALTER TABLE ${quoteName(name)}\n  ${ready.map((s) => "ADD COLUMN IF NOT EXISTS " + columnSql(s, w.src, models)).join(",\n  ")}`;
    const go = h("button", { class: "btn primary", onclick: async () => {
      go.disabled = true;
      try {
        await api("/v1/sql", { body: { sql } });
        toast(`Added ${plural(ready.length, "field")} to ${name}`, "ok");
        for (const s of ready) names.add(s.name.toLowerCase());
        const added = ready.map((s) => ({ ...s }));
        w.steps = w.steps.filter((s) => !ready.includes(s)); save();
        invalidate(); await tables(true);
        env.redraw({ fill: added.filter((s) => !fillsItself(s)).map((s) => s.name), src: w.src });
      } catch (e) { toast(e.message, "bad", 9000); go.disabled = false; }
    } }, icon("plus"), `Add ${plural(ready.length, "field")}`);
    gate(go, "admin");
    const llm = ready.some((s) => s.kind === "llm");
    clear(s4, h("div", { class: "code-wrap" }, sqlBlock(sql), h("button", { class: "btn xs code-copy", title: "Copy", onclick: () => copy(sql, "SQL copied") }, icon("copy"), "Copy")),
      h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } }, go,
        h("span", { class: "small dim" }, llm ? "A language model's field asks the model once a row written: a table loaded with a million rows makes a million requests." : "A table with a derived field needs this version of ChloeDB or later to open.")));
  }

  // ---- what the table's fields made so far
  async function drawMade(fill) {
    const derived = fieldsOf(env.table()).filter((f) => f.derive || f.embed);
    if (!derived.length) { clear(made); return; }
    const T0 = quoteName(name);
    const box = h("div", { class: "stack", style: { gap: "8px" } }, h("div", { class: "small dim" }, "…"));
    clear(made, card({ title: "Fields made of its rows", sub: `${name}: how many rows have each, and its commonest values`, body: box }));
    // COUNT(field) of a KEYWORD[] counts its values: a row with one is
    // counted by a filter instead.
    let counts;
    try {
      const one = derived.filter((f) => f.type !== "vector");
      const n = (sql) => api("/v1/query", { body: { sql } }).then((j) => j.rows?.[0]?.[0] || 0);
      counts = await Promise.all([n(`SELECT COUNT(*) FROM ${T0}`), ...one.map((f) => n(`SELECT COUNT(*) FROM ${T0} WHERE ${quoteName(f.name)} IS NOT NULL`))]);
    } catch (e) { clear(box, failure(e, "The counts")); return; }
    const total = counts[0] || 0;
    let schema = null;
    try { schema = await api(`/v1/schema?table=${encodeURIComponent(name)}`); } catch { /* the counts alone */ }
    let ci = 1;
    const rows = derived.map((f) => {
      const src = f.derive ? [].concat(f.derive.from).join(", ") : [].concat(f.embed.from).join(", ");
      const model = f.derive ? f.derive.model : f.embed.model;
      let have, note = null, act = null;
      if (f.type === "vector") {
        const b = (schema?.embeddings || []).find((e) => e.field === f.name)?.backfill;
        have = b ? Math.max(0, total - (b.missing - b.embedded)) : null;
        note = b ? (b.state === "running" ? "the server is filling in vectors" : b.state === "failed" ? `filling in failed: ${b.error}` : null) : null;
      } else {
        have = counts[ci++];
        const missing = total - have;
        if (missing > 0 && key) {
          // A field filled in here before: the rows left have no value of
          // their own (a text that names none), so filling in is a retry.
          const filled = (st.filled || {})[`${name}.${f.name}`];
          if (filled) note = `${plural(missing, "row")} with none after filling in`;
          act = gate(h("button", { class: filled ? "btn xs ghost" : "btn xs", onclick: () => fillIn([f.name], [].concat(f.derive.from)[0], f.derive.model) }, filled ? "Fill in again" : `Fill in ${plural(missing, "row")}`), "write");
        }
      }
      const pct = total && have != null ? Math.round((have / total) * 100) : null;
      const tops = h("div", { class: "row", style: { flexWrap: "wrap", gap: "4px" } });
      if (f.type === "keyword" && have) {
        api("/v1/query", { body: { sql: `SELECT ${quoteName(f.name)}, COUNT(*) AS n FROM ${T0} WHERE ${quoteName(f.name)} IS NOT NULL GROUP BY ${quoteName(f.name)} ORDER BY n DESC LIMIT 8` } })
          .then((j) => clear(tops, (j.rows || []).map((r) => h("span", { class: "chip", title: `${r[1]} rows` }, `${clip(r[0], 28)} · ${r[1]}`)))).catch(() => {});
      }
      return h("tr", {},
        h("td", { class: "mono" }, f.name + (f.multi ? "[]" : "")),
        h("td", { class: "small" }, `${model} of ${src}`),
        h("td", { class: "num" }, have == null ? "—" : `${pct}%`),
        h("td", {}, tops, note ? h("div", { class: "small dim" }, note) : null),
        h("td", {}, act));
    });
    clear(box, h("div", { class: "tbl-wrap" }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, h("th", {}, "field"), h("th", {}, "made by"), h("th", {}, "rows"), h("th", {}, "commonest"), h("th", {}, ""))), h("tbody", {}, rows))),
      h("div", { class: "small dim" }, `${plural(total, "row")}. A derived field is made when a row is written; rows written before it was added have none until they are filled in, which writes each one's ${"source"} again.`));
    if (fill?.fill?.length) fillIn(fill.fill, fill.src);
  }

  /** Writes the source of the rows that lack `fields` again, 200 at a time, so the writer makes them. */
  async function fillIn(fields, src, model) {
    if (!key) return toast(`${name} has no key: its rows can only be written again by hand`, "warn");
    const T0 = quoteName(name), K = quoteName(key), S = quoteName(src);
    const lacking = `(${fields.map((f) => `${quoteName(f)} IS NULL`).join(" OR ")}) AND ${S} IS NOT NULL`;
    let total;
    try { total = (await api("/v1/query", { body: { sql: `SELECT COUNT(*) FROM ${T0} WHERE ${lacking}` } })).rows?.[0]?.[0] || 0; } catch (e) { toast(e.message, "bad"); return; }
    if (!total) { toast("Every row has them", "ok"); return drawMade(); }
    const llm = fieldsOf(env.table()).some((f) => fields.includes(f.name) && f.type === "text" && f.derive && !/^(blip|whisper)/.test(f.derive.model)) || /\//.test(model || "");
    const ok = await confirm({ title: `Fill in ${plural(total, "row")} of ${name}?`, body: `Each row's ${src} is written again, ${Math.min(200, total)} rows a write, and the writer makes ${fields.join(", ")} of it as for a new row.${llm ? ` That is ${plural(total, "request")} to the language model.` : ""} You can stop it; the rows done keep their values.`, ok: "Fill in" });
    if (!ok) return;
    let stop = false, done = 0, after = null;
    const bar = h("div", { class: "ex-prog", style: { "--w": "0%" } });
    const label = h("span", { class: "small" }, `0 of ${total}`);
    const stopBtn = h("button", { class: "btn xs", onclick: () => { stop = true; stopBtn.disabled = true; } }, "Stop");
    const box = h("div", { class: "row", style: { gap: "10px" } }, h("b", { class: "small" }, `Filling in ${fields.join(", ")}`), bar, label, stopBtn);
    made.prepend(box);
    try {
      while (!stop) {
        const page = await api("/v1/query", { body: { sql: `SELECT ${K} FROM ${T0} WHERE ${lacking}${after != null ? ` AND ${K} > :after` : ""} ORDER BY ${K} LIMIT 200`, params: after != null ? { after } : {} } });
        const keys = (page.rows || []).map((r) => r[0]);
        if (!keys.length) break;
        await api("/v1/sql", { body: { sql: `UPDATE ${T0} SET ${S} = ${S} WHERE ${K} IN (${keys.map(litv).join(", ")})` } });
        done += keys.length; after = keys[keys.length - 1];
        bar.style.setProperty("--w", `${Math.min(100, Math.round((done / total) * 100))}%`);
        label.textContent = `${fmt.n ? fmt.n(done) : done} of ${fmt.n ? fmt.n(total) : total}`;
      }
      if (!stop) { st.filled = { ...(st.filled || {}), ...Object.fromEntries(fields.map((f) => [`${name}.${f}`, Date.now()])) }; save(); }
      toast(stop ? `Stopped after ${plural(done, "row")}` : `Filled in ${plural(done, "row")}`, "ok");
    } catch (e) { toast(`Filling in: ${e.message}`, "bad", 9000); }
    box.remove();
    drawMade();
  }

  return { fill: (x) => drawMade(x) };
}

// ======================================================================
// Enrich queries
// ======================================================================

/**
 * The query workflow. env: {body, st, save, table(), field, vector,
 * classifiers(), synonyms()}.
 */
export function enrichQueries(env) {
  const { body, st, save } = env;
  const T = env.table(), name = T.table, F = st.field, key = T.short?.key;
  const fs = fieldsOf(T);
  const q = st.qx && st.qx.table === name ? st.qx : (st.qx = { table: name, q: st.q || "", context: "", on: { spell: true, values: true, cls: true, lang: false, names: true, syn: false, expand: false, typos: false, meaning: false }, mode: "boost", ufields: null });
  if (q.ufields == null) {
    const nums = fs.filter((f) => (f.type === "int" || f.type === "float") && /price|cost|amount/i.test(f.name));
    q.ufields = [...fs.filter((f) => f.type === "keyword" && !f.derive && f.name !== key), ...nums.slice(0, 1)].map((f) => f.name).join(", ");
  }
  const classFields = fs.filter((f) => f.type === "keyword" && f.derive && /^classifier:/.test(f.derive.model));
  const langField = fs.find((f) => f.type === "keyword" && f.derive?.model === "builtin:lang");
  const dictFields = fs.filter((f) => f.type === "keyword" && f.derive && /^dict:/.test(f.derive.model));
  const vec = fs.find((f) => f.type === "vector" && f.embed && !f.multi && f.name === env.vector) || fs.find((f) => f.type === "vector" && f.embed && !f.multi);
  const hasSyn = (env.synonyms()?.rules || 0) > 0 || !!(env.synonyms()?.tables || []).find((x) => x.table.toLowerCase() === name.toLowerCase());

  const input = h("input", { class: "input qu-q", placeholder: "A search, as a person types it: waterprof hiking boots under 200", value: q.q });
  const ctxIn = h("textarea", { class: "input mono", rows: 2, placeholder: "What the caller knows, a field = value a line: country = CL", style: { width: "100%" } }, q.context || "");
  const steps = h("ol", { class: "start-steps ex-steps" });
  const out = h("div", { class: "stack" });
  const go = () => { q.q = input.value.trim(); q.context = ctxIn.value; save(); if (q.q) run(); };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
  clear(body,
    card({ body: h("div", { class: "stack", style: { gap: "8px" } },
      h("div", { class: "row nw" }, input, h("button", { class: "btn primary", onclick: go }, icon("sparkle"), "Read it")),
      h("details", { class: "mm-more", open: !!q.context }, h("summary", { class: "small" }, "Context: what the caller knows besides the words"), ctxIn,
        h("div", { class: "small dim" }, `Each line filters on a field of ${name}: the person's country, language or plan, the page's category. In a saved search it is a parameter the code passes.`))) }),
    h("div", { class: "qu-grid" }, steps, h("div", { class: "ex-sticky" }, out)));
  if (q.q) run(); else drawSteps(null);

  /** The step list, each with what it read; r is the run's readings (or null before one). */
  function drawSteps(r) {
    const on = q.on;
    let n = 0;
    const item = (id, title, what, read, avail = true, extra = null) => {
      n++;
      const t = avail ? toggle(on[id], (v) => { on[id] = v; save(); run(); }, "") : h("span", { class: "small dim" }, "not available");
      return h("li", { class: avail && on[id] ? "" : "off", "aria-disabled": String(!avail) },
        h("span", { class: "start-n" }, avail && on[id] && read ? icon("check") : String(n)),
        h("div", { class: "grow stack", style: { gap: "4px", minWidth: 0 } }, h("div", { class: "row", style: { gap: "10px", flexWrap: "wrap" } }, h("b", {}, title), t), h("div", { class: "small dim" }, what), avail && on[id] ? extra : null, avail && on[id] ? read : null));
    };
    const reading = (x) => (x == null ? null : h("div", { class: "row", style: { flexWrap: "wrap", gap: "4px" } }, x));
    const chips = (xs) => xs.map((x) => h("span", { class: "chip" }, x));
    const uf = h("input", { class: "input sm", value: q.ufields, title: "The fields whose values or ranges the words may name", style: { minWidth: "200px" }, onchange: () => { q.ufields = uf.value; save(); run(); } });
    const mode = seg([["boost", "boost"], ["filter", "filter"]], q.mode, (v) => { q.mode = v; save(); run(); });
    clear(steps,
      item("spell", "Spelling", "Words in none of the field's rows become the likeliest close word that is (/v1/correct): read before the request.",
        reading(r?.spell ? (r.spell.changed ? chips(r.spell.words.map((x) => `${x.word} → ${x.to_word}`)) : [h("span", { class: "small dim" }, "nothing to correct")]) : null)),
      item("values", "Values the words name", "Words that name a value of a field, or a range of a number (under 200), become a filter or a boost; BM25 ranks the rest. The query does this itself.",
        reading(r?.und ? [...chips([...(r.und.filters || []), ...(r.und.boosts || [])].map(String)), r.und.ranked != null ? h("span", { class: "small dim" }, `ranked by: ${r.und.ranked || "(nothing left)"}`) : null] : null),
        !!q.ufields, h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } }, h("span", { class: "small dim" }, "fields"), uf, mode)),
      item("cls", "Class", classFields.length ? `A classifier's label of the query, filtering ${classFields.map((f) => f.name).join(", ")}: read before the request (/v1/classify).` : "Needs a class field: add one under Enrich rows (a classifier's label of each row), then the query's class filters it.",
        reading(r?.cls ? r.cls.map((c) => h("span", { class: "chip" }, `${c.field} = ${c.label ?? "(unsure)"}${c.score != null ? ` · ${Math.round(c.score * 100)}%` : ""}`)) : null), classFields.length > 0),
      item("lang", "Language", langField ? `The query's language, filtering ${langField.name}. Short queries are often unsure, so it starts off.` : "Needs a language field: add one under Enrich rows.",
        reading(r?.lang ? chips([`${langField.name} = ${r.lang}`]) : null), !!langField),
      item("names", "Names it mentions", dictFields.length ? `The values of ${dictFields.map((f) => f.derive.model.slice(5)).join(", ")} it names in full, filtering ${dictFields.map((f) => f.name).join(", ")}.` : "Needs a field of names: add one under Enrich rows (Names it mentions).",
        reading(r?.names ? (r.names.length ? chips(r.names.map((n) => `${n.field} = ${n.value}`)) : [h("span", { class: "small dim" }, "names none")]) : null), dictFields.length > 0),
      item("syn", "Synonyms", hasSyn ? "The table's synonym list: sofa also finds couch." : "No synonym list: make or mine one under Configure.", null, hasSyn),
      item("typos", "Typos", "Each word also matches words an edit or two away, inside the query (fuzzy => 'auto').", null),
      item("expand", "Expansion", "Words of the top five rows join the query (RM3): more recall, sometimes off topic.", null),
      item("meaning", "Meaning", vec ? `BM25 fused with similarity of ${vec.name} (${vec.embed.model}), so rows that say it in other words come up.` : "Needs a vector field: add one under Enrich rows (Vector for meaning).", null, !!vec));
  }

  /** The context lines as filters on fields that exist. */
  function contextFilters() {
    const out = [];
    for (const line of (q.context || "").split("\n")) {
      const m = /^\s*([A-Za-z_][\w]*)\s*=\s*(.+?)\s*$/.exec(line);
      if (!m) continue;
      const f = fs.find((x) => x.name.toLowerCase() === m[1].toLowerCase());
      if (!f) continue;
      let v = m[2].replace(/^'(.*)'$/, "$1");
      if (f.type === "int" || f.type === "float") v = Number(v);
      out.push({ field: f.name, value: v, ty: f.type === "int" ? "INT" : f.type === "float" ? "FLOAT" : "KEYWORD", from: "context" });
    }
    return out;
  }

  async function run() {
    clear(out, h("div", { class: "small dim" }, "Reading the query…"));
    const on = q.on, r = {};
    let text = q.q;
    const jobs = [];
    if (on.spell) jobs.push(api("/v1/correct", { body: { table: name, field: F, text } }).then((j) => { r.spell = j; if (j.changed) text = j.text; }).catch((e) => { r.spellErr = e; }));
    await Promise.all(jobs);
    const reads = [];
    if (on.values && q.ufields) reads.push(api("/v1/understand", { body: { table: name, field: F, text, fields: q.ufields, mode: q.mode } }).then((j) => { r.und = j; }).catch((e) => { r.undErr = e; }));
    if (on.cls && classFields.length) {
      r.cls = [];
      for (const f of classFields) {
        const c = f.derive.model.slice("classifier:".length);
        reads.push(api("/v1/classify", { body: { name: c, text, k: 3 } }).then((j) => { r.cls.push({ field: f.name, label: j.label, score: j.score, multi: !!f.multi }); }).catch((e) => { r.cls.push({ field: f.name, label: null, error: e.message }); }));
      }
    }
    if (on.lang && langField) reads.push(api("/v1/derive/preview", { body: { model: "builtin:lang", text } }).then((j) => { r.lang = j.values[0] || null; }).catch(() => {}));
    if (on.names && dictFields.length) {
      r.names = [];
      for (const f of dictFields) reads.push(api("/v1/derive/preview", { body: { model: f.derive.model, text, table: name, field: [].concat(f.derive.from)[0] } }).then((j) => { for (const v of j.values) r.names.push({ field: f.name, value: v }); }).catch(() => {}));
    }
    await Promise.all(reads);
    drawSteps(r);

    // The features read before the request become filters, each a
    // parameter; the rest are options of bm25 the query runs itself.
    const filters = [];
    for (const c of r.cls || []) if (c.label != null) filters.push({ field: c.field, value: c.label, ty: "KEYWORD", from: "class" });
    if (r.lang && r.lang !== "und") filters.push({ field: langField.name, value: r.lang, ty: "KEYWORD", from: "language" });
    for (const n of (r.names || []).slice(0, 3)) filters.push({ field: n.field, value: n.value, ty: "KEYWORD", from: "names" });
    filters.push(...contextFilters());
    filters.forEach((f, i) => { f.param = ident(`${f.field}${filters.slice(0, i).some((x) => x.field === f.field) ? "_" + i : ""}`); });

    const opts = [
      on.values && q.ufields ? `understand => '${q.mode}', understand_fields => ${lit(q.ufields)}` : null,
      on.syn && hasSyn ? "synonyms => 1" : null,
      on.typos ? "fuzzy => 'auto'" : null,
      on.expand ? "expand => 5" : null,
    ].filter(Boolean);
    const T0 = quoteName(name), Fq = quoteName(F);
    const show = fs.find((f) => f.type === "text" && isStored(f) && f.name !== key)?.name;
    const cols = [key, show].filter(Boolean).map(quoteName).join(", ") || "*";
    const bm = `bm25(${Fq}, :q${opts.length ? ", " + opts.join(", ") : ""})`;
    const rank = on.meaning && vec ? `rrf(${bm}, similarity(${quoteName(vec.name)}, :q))` : bm;
    const where = (optional) => filters.length ? ` WHERE ${filters.map((f) => optional ? `${quoteName(f.field)} = coalesce(:${f.param}, ${quoteName(f.field)})` : `${quoteName(f.field)} = :${f.param}`).join(" AND ")}` : "";
    const sqlOf = (optional) => `SELECT ${cols} FROM ${T0}${where(optional)} ORDER BY ${rank} DESC LIMIT 10`;
    const plainSql = `SELECT ${cols} FROM ${T0} ORDER BY bm25(${Fq}, :q) DESC LIMIT 10`;
    const finalSql = sqlOf(false);
    const params = { q: text, ...Object.fromEntries(filters.map((f) => [f.param, f.value])) };

    const res = h("div", { class: "small dim" }, "…");
    const features = [
      text !== q.q ? h("span", { class: "chip" }, `text: ${text}`) : null,
      ...filters.map((f) => h("span", { class: "chip", title: `from ${f.from}` }, `${f.field} = ${f.value}`)),
      ...opts.map((o) => h("span", { class: "chip mono" }, o.split(" =>")[0])),
      on.meaning && vec ? h("span", { class: "chip mono" }, "similarity") : null,
    ].filter(Boolean);
    const saveBtn = h("button", { class: "btn", onclick: () => saveSearch({
      suggested: `search_${name}`,
      description: `Searches ${name} by ${F}${opts.length ? " with " + opts.map((o) => o.split(" =>")[0]).join(", ") : ""}${filters.length ? `, filtered by ${filters.map((f) => f.field).join(", ")} when given` : ""}`,
      params: [{ name: "q", ty: "TEXT" }, ...filters.map((f) => ({ name: f.param, ty: f.ty, default: "NULL" }))],
      body: sqlOf(true),
      table: name,
    }) }, icon("bookmark"), "Save as a search");
    gate(saveBtn, "admin");
    const code = h("div");
    clear(out,
      card({ title: "The request", sub: features.length ? "what the steps added" : "nothing added: the words alone", body: h("div", { class: "stack", style: { gap: "8px" } },
        features.length ? h("div", { class: "row", style: { flexWrap: "wrap", gap: "4px" } }, features) : null,
        h("div", { class: "code-wrap" }, sqlBlock(finalSql), h("button", { class: "btn xs code-copy", title: "Copy", onclick: () => copy(finalSql, "SQL copied") }, icon("copy"), "Copy")),
        h("div", { class: "small dim mono" }, "params ", JSON.stringify(params)),
        h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } }, saveBtn,
          h("button", { class: "btn ghost", onclick: () => { clear(code, card({ title: "From your code", sub: "the request with what this query said", body: useFromCode({ sql: finalSql, params }) })); } }, icon("sql"), "Call it from code"),
          filters.length ? h("span", { class: "small dim" }, "Saved, each filter is a parameter (NULL leaves it out): the code reads spelling, class, language and names of each query first, as this page does, and passes them.") : null)) }),
      code,
      card({ title: "Top rows", sub: "the words alone, beside the request", body: res }));
    const runQ = (sql, p) => api("/v1/query", { body: { sql, params: p } }).catch((e) => ({ error: e }));
    const [a, b] = await Promise.all([runQ(plainSql, { q: q.q }), runQ(finalSql, params)]);
    const ids = (j) => new Set((j?.rows || []).map((x) => String(x[0])));
    const list = (j, other) => j?.error ? failure(j.error, "The query") : !j.rows?.length ? h("div", { class: "small dim" }, "No row matches.") : h("ol", { class: "qu-rows" }, j.rows.map((x) => {
      const id = String(x[0]);
      return h("li", { class: other && !other.has(id) ? "new" : "" }, h("span", { class: "mono small" }, clip(id, 24)), x.length > 1 ? h("span", { class: "small dim" }, " " + clip(x[1], 90)) : null);
    }));
    const plan = (j) => (j?.stats?.plan || []).filter((l) => /understood|expanded|synonym|fused|fuzzy|typo/.test(l)).slice(0, 4);
    clear(res, h("div", { class: "qu-cmp" },
      h("div", { class: "stack", style: { gap: "6px" } }, h("b", { class: "small" }, `The words alone: “${q.q}”`), list(a, ids(b))),
      h("div", { class: "stack", style: { gap: "6px" } }, h("b", { class: "small" }, "The request"), list(b, ids(a)), ...plan(b).map((l) => h("div", { class: "small dim mono" }, l)))));
  }
}

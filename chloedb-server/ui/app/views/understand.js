// Query and content understanding: what the database's understanding
// components do to a search and to a row, and where they are configured.
//
// - Explore a query: "did you mean" (/v1/correct), the words that name a
//   table's values (/v1/understand), each classifier's classes of the text
//   (/v1/classify), and the top rows of plain BM25 beside the same search
//   with understanding, expansion, synonyms or typo tolerance on, with
//   the plan lines that say what each did.
// - Preview a row: its text, its derived KEYWORD fields (languages,
//   keyphrases, near duplicates, entities, classes), each classifier's
//   class of its vector, and its chunks (`chunks_of`) at a size chosen.
// - Configure: the table's classifiers (CREATE CLASSIFIER, DROP), its own
//   synonym list (CREATE SYNONYMS) and synonyms mined from it
//   (/v1/synonyms/mine), proposed for the list and never applied by
//   themselves.

import { h, clear, api, store, empty, seg, failure, sqlBlock, toggle, card, gate, toast, confirm, fmt } from "../core.js";
import { tables, table as tableOf } from "../catalog.js";
import { quoteName } from "../sqledit.js";

export default { id: "understand", title: "Query understanding", icon: "sparkle", group: "Labs", full: true, range: false, keywords: "query understanding content understanding classify classifier synonyms mine spell correct did you mean chunks chunking derive keyphrases language entities expand rm3 understand boost filter", render };

const lit = (v) => `'${String(v).replace(/'/g, "''")}'`;
const fieldsOf = (t) => t?.short?.fields || [];
const stored = (f) => f.stored ?? !(f.from && f.from.length);
const allText = (t) => fieldsOf(t).filter((f) => f.type === "text");
const textFields = (t) => allText(t).filter((f) => f.indexed !== false);
const storedText = (t) => allText(t).filter(stored);
const vectorFields = (t) => fieldsOf(t).filter((f) => f.type === "vector" && !f.multi);
const keywordFields = (t) => fieldsOf(t).filter((f) => f.type === "keyword" && f.name !== t.short?.key);
const labelFields = (t) => fieldsOf(t).filter((f) => ["keyword", "text", "int", "bool"].includes(f.type) && !f.multi && f.name !== t.short?.key && !f.derive)
  .sort((a, b) => (a.type === "keyword" ? 0 : 1) - (b.type === "keyword" ? 0 : 1));
const derivedKeywords = (t) => fieldsOf(t).filter((f) => f.type === "keyword" && f.derive);
const clip = (s, n = 140) => { s = String(s ?? ""); return s.length > n ? s.slice(0, n - 1) + "…" : s; };

function render(page) {
  let st = store.get("qu.state", {}) || {};
  const save = () => store.set("qu.state", st);
  const top = h("div", { class: "qu-top" });
  const body = h("div", { class: "qu-body" });
  page.append(h("div", { class: "qu" }, top, body));
  let classifiers = [], synonyms = null;

  const t = () => tableOf(st.table);
  // The stored text chunks and mining read: the searched field when it is
  // stored, else the table's longest-sounding stored text (a description).
  const storedField = () => { const ss = storedText(t()); return (ss.find((f) => f.name === st.field) || ss.find((f) => /desc|body|text|content|abstract|summary/i.test(f.name)) || ss[0])?.name || null; };
  async function loadAux() {
    const [c, s] = await Promise.all([api("/v1/classifiers").catch(() => ({ classifiers: [] })), api("/v1/synonyms").catch(() => null)]);
    classifiers = c.classifiers || [];
    synonyms = s;
  }
  const mine = () => classifiers.filter((c) => c.table.toLowerCase() === st.table.toLowerCase());
  const tableList = () => (synonyms?.tables || []).find((x) => x.table.toLowerCase() === st.table.toLowerCase());

  Promise.all([tables(), loadAux()]).then(([ts]) => {
    const usable = ts.filter((x) => textFields(x).length && !x.table.startsWith("_"));
    if (!usable.length) { clear(body, empty("sparkle", "No text to understand", "Understanding reads a table's TEXT fields: make a table with one, then come back.")); return; }
    if (!tableOf(st.table) || !textFields(tableOf(st.table)).length) st.table = usable[0].table;
    fixFields();
    st.tab = st.tab || "explore";
    save(); drawTop(); draw();
    function drawTop() {
      const tsel = h("select", { class: "select", onchange: () => { st.table = tsel.value; st.field = st.vector = null; fixFields(); save(); drawTop(); draw(); } }, usable.map((x) => h("option", { value: x.table, selected: x.table === st.table }, x.table)));
      const fsel = h("select", { class: "select", title: "The text field searches read", onchange: () => { st.field = fsel.value; save(); draw(); } }, textFields(t()).map((f) => h("option", { value: f.name, selected: f.name === st.field }, f.name)));
      const vs = vectorFields(t());
      const vsel = vs.length ? h("select", { class: "select", title: "The vector field classifiers and mining embed with", onchange: () => { st.vector = vsel.value; save(); draw(); } }, vs.map((f) => h("option", { value: f.name, selected: f.name === st.vector }, f.name))) : null;
      const tabs = seg([["explore", "Explore a query"], ["preview", "Preview rows"], ["configure", "Configure"]], st.tab, (v) => { st.tab = v; save(); draw(); });
      clear(top,
        h("label", { class: "row nw" }, h("span", { class: "small dim" }, "Table"), tsel),
        h("label", { class: "row nw" }, h("span", { class: "small dim" }, "Text"), fsel),
        vsel ? h("label", { class: "row nw" }, h("span", { class: "small dim" }, "Vectors"), vsel) : null,
        h("div", { class: "qu-spacer" }), tabs);
    }
  }).catch((e) => clear(body, failure(e, "The tables")));

  function fixFields() {
    const x = t();
    if (!textFields(x).some((f) => f.name === st.field)) st.field = (textFields(x).find(stored) || textFields(x)[0])?.name;
    if (!vectorFields(x).some((f) => f.name === st.vector)) st.vector = vectorFields(x)[0]?.name || null;
  }

  function draw() {
    if (st.tab === "preview") return drawPreview();
    if (st.tab === "configure") return drawConfigure();
    return drawExplore();
  }

  // ------------------------------------------------------------ explore
  function drawExplore() {
    const input = h("input", { class: "input qu-q", placeholder: "Type a search, as a person would: blue velvet sofa under 500", value: st.q || "" });
    const opts = st.opts || (st.opts = { understand: true, expand: false, synonyms: false, fuzzy: false });
    const T = t();
    if (st.ufieldsFor !== st.table || st.ufieldsV !== 2) {
      st.ufieldsV = 2;
      // The keyword fields, and the numbers a search names a range of.
      // One number at most: "under 200" names no field when two could take it.
      const nums = fieldsOf(T).filter((f) => (f.type === "int" || f.type === "float") && /price|cost|amount/i.test(f.name));
      st.ufields = [...keywordFields(T).filter((f) => !f.derive), ...nums.slice(0, 1)].map((f) => f.name).join(", ");
      st.ufieldsFor = st.table;
    }
    const uf = h("input", { class: "input sm qu-uf", value: st.ufields, title: "The fields whose values or ranges the words may name", oninput: () => { st.ufields = uf.value; save(); } });
    const out = h("div", { class: "qu-grid" });
    const go = () => { st.q = input.value.trim(); save(); if (st.q) explore(st.q, out); };
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") go(); });
    const hasSyn = (synonyms?.rules || 0) > 0 || !!tableList();
    const toggles = h("div", { class: "row" },
      toggle(opts.understand, (v) => { opts.understand = v; save(); }, "understand (boost)"), uf,
      toggle(opts.expand, (v) => { opts.expand = v; save(); }, "expand (RM3, 5 rows)"),
      hasSyn ? toggle(opts.synonyms, (v) => { opts.synonyms = v; save(); }, "synonyms") : h("span", { class: "small dim", title: "No synonym list: make one under Configure" }, "no synonym list"),
      toggle(opts.fuzzy, (v) => { opts.fuzzy = v; save(); }, "typos (auto)"));
    clear(body,
      card({ body: h("div", { class: "qu-col" }, h("div", { class: "row nw" }, input, h("button", { class: "btn primary", onclick: go }, "Understand")), toggles) }),
      out);
    if (st.q) explore(st.q, out);
  }

  async function explore(q, out) {
    const T = t(), F = st.field;
    const understandFields = (st.ufields || "").split(",").map((x) => x.trim()).filter(Boolean).join(", ");
    const left = h("div", { class: "qu-col" }), right = h("div", { class: "qu-col" });
    clear(out, left, right);
    // Did you mean.
    const corr = card({ title: "Did you mean", sub: "/v1/correct", body: h("div", { class: "small dim" }, "…") });
    // Understood words.
    const und = card({ title: "Words that name values", sub: "/v1/understand", body: h("div", { class: "small dim" }, "…") });
    // Classes.
    const cls = card({ title: "Classes", sub: "/v1/classify", body: h("div", { class: "small dim" }, "…") });
    left.append(corr, und, cls);
    const res = card({ title: "Top rows", sub: "plain BM25 beside the search with the components on", body: h("div", { class: "small dim" }, "…") });
    right.append(res);

    api("/v1/correct", { body: { table: st.table, field: F, text: q } }).then((j) => {
      const b = corr.querySelector(".bd");
      if (!j.changed) { clear(b, h("div", { class: "small" }, "Every word is in the field's terms, or no neighbour is likely enough: nothing to correct.")); return; }
      clear(b, h("div", { class: "qu-dym" }, "Did you mean ", h("b", {}, j.text), "?"),
        h("div", { class: "small dim" }, j.words.map((w) => `${w.word} → ${w.to_word} (${w.edits} edit${w.edits === 1 ? "" : "s"}, ${Math.round(w.chance * 100)}% likely)`).join(", ")));
    }).catch((e) => clear(corr.querySelector(".bd"), failure(e, "Correction")));

    if (understandFields) {
      api("/v1/understand", { body: { table: st.table, field: F, text: q, fields: understandFields } }).then((j) => {
        const b = und.querySelector(".bd");
        const vals = (j.spans || []).map((sp) => h("div", { class: "row" }, h("span", { class: "small" }, h("b", {}, `“${sp.words}”`), sp.share != null ? h("span", { class: "dim" }, ` ${Math.round(sp.share * 100)}% of its rows`) : null),
          ...sp.matches.slice(0, 6).map((m) => h("span", { class: "chip", title: `${m.rows} rows${m.full ? ", named in full" : ""}` }, `${m.field} = ${m.value}`))));
        const bound = (b, lt) => b ? `${lt ? (b[1] ? "≤" : "<") : (b[1] ? "≥" : ">")} ${b[0]}` : null;
        const ranges = (j.ranges || []).map((r) => h("span", { class: "chip", title: `“${r.words}”` }, `${r.field} ${[bound(r.lo, false), bound(r.hi, true)].filter(Boolean).join(" and ")}`));
        clear(b,
          h("div", { class: "small" }, "Ranked by BM25: ", h("b", {}, j.ranked || "(nothing left)")),
          vals.length || ranges.length ? h("div", { class: "qu-col" }, ...vals, ranges.length ? h("div", { class: "row" }, ...ranges) : null) : h("div", { class: "small dim" }, "No word names a value of " + understandFields),
          j.line ? h("div", { class: "small dim mono" }, j.line) : null);
      }).catch((e) => clear(und.querySelector(".bd"), failure(e, "Understanding")));
    } else clear(und.querySelector(".bd"), h("div", { class: "small dim" }, "The table has no KEYWORD or number field for words to name."));

    const cs = mine();
    if (!cs.length) clear(cls.querySelector(".bd"), h("div", { class: "small dim" }, "No classifier of this table yet: train one under Configure."));
    else Promise.all(cs.map((c) => api("/v1/classify", { body: { name: c.name, text: q, k: 3 } }).then((j) => ({ c, j })).catch((e) => ({ c, e })))).then((rs) => {
      clear(cls.querySelector(".bd"), ...rs.map(({ c, j, e }) => h("div", { class: "qu-cls" },
        h("div", { class: "small" }, h("b", {}, c.name), h("span", { class: "dim" }, ` of ${c.labels_from} (${c.labels} labels)`)),
        e ? failure(e, c.name) : h("div", { class: "qu-bars" }, (j.top || []).map(([l, p]) => h("div", { class: "qu-bar" }, h("span", { class: "qu-bar-l" }, l), h("span", { class: "qu-bar-v", style: { "--w": `${Math.round(p * 100)}%` } }), h("span", { class: "small dim" }, `${Math.round(p * 100)}%`)))))));
    });

    // Rankings: plain, then with what is on.
    const o = st.opts, T0 = quoteName(st.table), Fq = quoteName(F);
    const key = T.short?.key, show = storedText(T).find((f) => f.name !== key)?.name;
    const cols = [key, show].filter(Boolean).map(quoteName).join(", ") || "*";
    const extra = [o.understand && understandFields ? `understand => 'boost', understand_fields => ${lit(understandFields)}` : null, o.expand ? "expand => 5" : null, o.synonyms ? "synonyms => 1" : null, o.fuzzy ? "fuzzy => 'auto'" : null].filter(Boolean);
    const plainSql = `SELECT ${cols} FROM ${T0} ORDER BY bm25(${Fq}, :q) DESC LIMIT 10`;
    const withSql = extra.length ? `SELECT ${cols} FROM ${T0} ORDER BY bm25(${Fq}, :q, ${extra.join(", ")}) DESC LIMIT 10` : null;
    const run = (sql) => api("/v1/query", { body: { sql, params: { q } } });
    Promise.all([run(plainSql), withSql ? run(withSql) : null].map((p) => p && p.catch((e) => ({ error: e })))).then(([a, b]) => {
      const list = (j, other) => j?.error ? failure(j.error, "The query") : !j.rows?.length ? h("div", { class: "small dim" }, "No row matches.") : h("ol", { class: "qu-rows" }, (j.rows || []).map((r) => {
        const id = String(r[0]);
        const moved = other && !other.has(id);
        return h("li", { class: moved ? "new" : "" }, h("span", { class: "mono small" }, clip(id, 24)), r.length > 1 ? h("span", { class: "small dim" }, " " + clip(r[1], 90)) : null);
      }));
      const ids = (j) => new Set((j?.rows || []).map((r) => String(r[0])));
      const plan = (j) => (j?.stats?.plan || []).filter((l) => /^(bm25|classify|understood|expanded|synonym)/.test(l) || l.includes("understood") || l.includes("expanded")).slice(0, 4);
      clear(res.querySelector(".bd"), h("div", { class: "qu-cmp" },
        h("div", { class: "qu-col" }, h("b", { class: "small" }, "Plain BM25"), list(a, b && ids(b)), sqlBlock(plainSql)),
        b ? h("div", { class: "qu-col" }, h("b", { class: "small" }, "With " + extra.map((x) => x.split(" ")[0]).join(", ")), list(b, ids(a)),
          ...plan(b).map((l) => h("div", { class: "small dim mono" }, l)), sqlBlock(withSql)) : h("div", { class: "small dim" }, "Turn a component on to compare.")));
    });
  }

  // ------------------------------------------------------------ preview
  function drawPreview() {
    const T = t(), key = T.short?.key;
    if (!key) { clear(body, empty("table", "No key", "Previewing a row reads it by its key: this table has none.")); return; }
    const search = h("input", { class: "input", placeholder: "Find rows: words of the text, or a key", value: st.find || "" });
    const list = h("div", { class: "qu-picks" });
    const view = h("div", { class: "qu-col" });
    const max = h("input", { class: "input sm", type: "number", min: 16, max: 8192, value: st.maxTokens || 256, style: { width: "80px" } });
    const ovl = h("input", { class: "input sm", type: "number", min: 0, max: 4096, value: st.overlap || 0, style: { width: "70px" } });
    const find = async () => {
      st.find = search.value.trim(); save();
      const K = quoteName(key), T0 = quoteName(st.table), Fq = quoteName(st.field);
      const sql = st.find ? `SELECT ${K} FROM ${T0} WHERE MATCH(${Fq}, :q) OR ${K} = :q ORDER BY bm25(${Fq}, :q) DESC LIMIT 12` : `SELECT ${K} FROM ${T0} LIMIT 12`;
      try {
        const j = await api("/v1/query", { body: { sql, params: { q: st.find } } });
        clear(list, ...(j.rows || []).map((r) => h("button", { class: "chip", "aria-pressed": String(String(r[0]) === st.row), onclick: () => { st.row = String(r[0]); save(); find(); show(); } }, clip(r[0], 30))));
        if (!st.row && j.rows?.length) { st.row = String(j.rows[0][0]); show(); }
      } catch (e) { clear(list, failure(e, "Rows")); }
    };
    search.addEventListener("keydown", (e) => { if (e.key === "Enter") find(); });
    const rechunk = () => { st.maxTokens = +max.value || 256; st.overlap = +ovl.value || 0; save(); show(); };
    max.addEventListener("change", rechunk); ovl.addEventListener("change", rechunk);
    clear(body, h("div", { class: "qu-grid" },
      h("div", { class: "qu-col" }, card({ body: h("div", { class: "qu-col" }, search, list) }), view),
      h("div", { class: "qu-col" }, card({ title: "Chunks", sub: "chunks_of()", tools: h("div", { class: "row nw" }, h("span", { class: "small dim" }, "max tokens"), max, h("span", { class: "small dim" }, "overlap"), ovl), body: h("div", { class: "qu-chunks" }, h("div", { class: "small dim" }, "Pick a row.")) }))));
    find();
    if (st.row) show();

    async function show() {
      const K = quoteName(key), T0 = quoteName(st.table), F = st.field;
      const texts = storedText(T).map((f) => f.name);
      const derived = derivedKeywords(T);
      const cs = mine().filter((c) => vectorFields(T).some((v) => v.name === c.field));
      const cols = [...new Set([...texts, ...derived.map((f) => f.name)])].map(quoteName);
      const calls = cs.flatMap((c) => [`${quoteName(c.name)}(${quoteName(c.field)})`, `${quoteName(c.name)}(${quoteName(c.field)}, 'score')`]);
      const sql = `SELECT ${[K, ...cols, ...calls].join(", ")} FROM ${T0} WHERE ${K} = :id`;
      const SF = storedField();
      const chunksSql = SF ? `SELECT n, tokens, start_char, end_char, chunk FROM chunks_of(${T0}, ${quoteName(SF)}, max_tokens => ${st.maxTokens || 256}, overlap => ${st.overlap || 0}) WHERE ${K} = :id` : null;
      const chunkBox = body.querySelector(".qu-chunks");
      try {
        const j = await api("/v1/query", { body: { sql, params: { id: st.row } } });
        const r = j.rows?.[0];
        if (!r) { clear(view, empty("search", "No such row", st.row)); return; }
        let i = 1;
        const textCards = texts.map((n) => card({ title: n, sub: "text", body: h("div", { class: "qu-text" }, String(r[i++] ?? "")) }));
        const dv = derived.filter((f) => !texts.includes(f.name)).map((f) => {
          const v = r[i++];
          const vals = Array.isArray(v) ? v : v == null ? [] : [v];
          return h("div", { class: "qu-derived" }, h("div", { class: "small" }, h("b", {}, f.name), h("span", { class: "dim" }, ` ${f.derive.model} of ${[].concat(f.derive.from).join(", ")}`)),
            vals.length ? h("div", { class: "row" }, vals.map((x) => h("span", { class: "chip" }, clip(x, 40)))) : h("div", { class: "small dim" }, "(none: rows written before the field have no value)"));
        });
        const cv = cs.map((c) => { const label = r[i++], score = r[i++]; return h("div", { class: "qu-derived" }, h("div", { class: "small" }, h("b", {}, c.name), h("span", { class: "dim" }, ` classifier of ${c.labels_from}, v${c.version}`)), h("div", { class: "row" }, label == null ? h("span", { class: "small dim" }, "(no class)") : h("span", { class: "chip" }, String(label)), score != null ? h("span", { class: "small dim" }, `${Math.round(score * 100)}%`) : null)); });
        // What each built-in model would make of the stored text, whether
        // or not a field holds it yet.
        const SFv = SF ? r[1 + texts.indexOf(SF)] : null;
        const models = h("div", { class: "qu-col" }, h("div", { class: "small dim" }, "…"));
        clear(view, ...textCards,
          card({ title: "What the components say of it", sub: "derived fields and classifiers", body: dv.length || cv.length ? h("div", { class: "qu-col" }, ...dv, ...cv) : h("div", { class: "small dim" }, "No derived KEYWORD field and no classifier on this table yet: see Configure.") }),
          SFv ? card({ title: `What the built-in models make of ${SF}`, sub: "/v1/derive/preview", body: models }) : null,
          card({ title: "The query", body: sqlBlock(sql) }));
        if (SFv) Promise.all(["builtin:lang", "builtin:keyphrases", "builtin:minhash"].map((m) => api("/v1/derive/preview", { body: { model: m, text: String(SFv) } }).then((j) => ({ m, j })).catch((e) => ({ m, e })))).then((rs) => {
          const field = (m) => derived.find((f) => f.derive.model === m);
          clear(models, ...rs.map(({ m, j, e }) => h("div", { class: "qu-derived" },
            h("div", { class: "small" }, h("b", {}, m), field(m) ? h("span", { class: "dim" }, ` (held by ${field(m).name}${field(m).derive.from === SF ? "" : ", of " + field(m).derive.from})`) : h("span", { class: "dim" }, " (no field holds it)")),
            e ? failure(e, m) : h("div", { class: "row" }, (m === "builtin:minhash" ? j.values.slice(0, 4) : j.values).map((x) => h("span", { class: "chip mono" }, clip(x, 40))), m === "builtin:minhash" && j.values.length > 4 ? h("span", { class: "small dim" }, `+${j.values.length - 4} bands`) : null))));
        });
      } catch (e) { clear(view, failure(e, "The row")); }
      if (!chunksSql) { clear(chunkBox, h("div", { class: "small dim" }, "The table stores no text: chunks are of a stored TEXT field.")); return; }
      try {
        const c = await api("/v1/query", { body: { sql: chunksSql, params: { id: st.row } } });
        clear(chunkBox, h("div", { class: "small dim" }, `${c.rows.length} chunk${c.rows.length === 1 ? "" : "s"} of ${SF}`), ...c.rows.map((x) => h("div", { class: "qu-chunk" }, h("div", { class: "small dim" }, `#${x[0]} · ${x[1]} tokens · chars ${x[2]}–${x[3]}`), h("div", { class: "qu-text" }, String(x[4])))), sqlBlock(chunksSql));
      } catch (e) { clear(chunkBox, failure(e, "Chunks")); }
    }
  }

  // ------------------------------------------------------------ configure
  function drawConfigure() {
    const T = t();
    const left = h("div", { class: "qu-col" }), right = h("div", { class: "qu-col" });
    clear(body, h("div", { class: "qu-grid" }, left, right));
    left.append(classifierCard(T), derivedCard(T));
    right.append(synonymCard(T));
  }

  async function sql(text, what) {
    try { const j = await api("/v1/sql", { body: { sql: text } }); toast(what, "ok"); return j; } catch (e) { toast(`${what}: ${e.message}`, "error", 8000); throw e; }
  }

  function classifierCard(T) {
    const cs = mine();
    const listEl = cs.length ? h("table", { class: "t compact" },
      h("thead", {}, h("tr", {}, ...["name", "vectors", "labels of", "labels", "method", "held out right", ""].map((x) => h("th", {}, x)))),
      h("tbody", {}, cs.map((c) => {
        const drop = h("button", { class: "btn xs danger", onclick: async () => { if (await confirm({ title: `Drop classifier ${c.name}?`, body: "Its function is dropped (a tombstone version): queries that call it fail, and a derived field of it stays as written.", ok: "Drop", danger: true })) { await sql(`DROP CLASSIFIER ${quoteName(c.name)}`, `Dropped ${c.name}`); await loadAux(); draw(); } } }, "Drop");
        gate(drop, "admin");
        return h("tr", {}, h("td", { class: "mono" }, `${c.name} v${c.version}`), h("td", {}, c.field), h("td", {}, c.labels_from), h("td", { class: "num", title: (c.label_names || []).join(", ") }, String(c.labels)), h("td", {}, String(c.method)), h("td", { class: "num" }, c.holdout_accuracy == null ? "—" : `${(c.holdout_accuracy * 100).toFixed(1)}%`), h("td", {}, drop));
      }))) : h("div", { class: "small dim" }, "None yet.");
    const vs = vectorFields(T), ls = labelFields(T);
    if (!vs.length || !ls.length) return card({ title: "Classifiers", sub: "CREATE CLASSIFIER", body: h("div", { class: "qu-col" }, listEl, h("div", { class: "small dim" }, "A classifier learns a column's labels from a VECTOR field: this table needs both.")) });
    const f = st.cf || (st.cf = { name: "topic", vector: st.vector || vs[0].name, labels: ls[0].name, method: "logistic", where: "" });
    if (!vs.some((v) => v.name === f.vector)) f.vector = vs[0].name;
    if (!ls.some((v) => v.name === f.labels)) f.labels = ls[0].name;
    const name = h("input", { class: "input sm", value: f.name, style: { width: "120px" }, oninput: () => { f.name = name.value.trim(); show(); } });
    const vec = h("select", { class: "select sm", onchange: () => { f.vector = vec.value; show(); } }, vs.map((v) => h("option", { value: v.name, selected: v.name === f.vector }, v.name)));
    const lab = h("select", { class: "select sm", onchange: () => { f.labels = lab.value; show(); } }, ls.map((v) => h("option", { value: v.name, selected: v.name === f.labels }, v.name)));
    const where = h("input", { class: "input sm", placeholder: "WHERE (optional): rows to learn from", value: f.where, oninput: () => { f.where = where.value; show(); } });
    const method = seg([["logistic", "logistic"], ["centroid", "centroid"]], f.method, (v) => { f.method = v; show(); });
    const pre = h("div");
    const report = h("div");
    const text = () => `CREATE ${cs.some((c) => c.name.toLowerCase() === f.name.toLowerCase()) ? "OR REPLACE " : ""}CLASSIFIER ${quoteName(f.name || "topic")} ON ${quoteName(st.table)}(${quoteName(f.vector)}) LABELS ${quoteName(f.labels)} OPTIONS (method = '${f.method}')${f.where.trim() ? ` WHERE ${f.where.trim()}` : ""}`;
    const show = () => { save(); clear(pre, sqlBlock(text())); };
    const train = h("button", { class: "btn primary sm", onclick: async () => {
      train.disabled = true; clear(report, h("div", { class: "small dim" }, "Training…"));
      try {
        const j = await sql(text(), `Trained ${f.name}`);
        const row = j.rows?.[0] || [], cols = j.columns || [];
        clear(report, h("div", { class: "row" }, cols.map((c, i) => h("span", { class: "chip" }, `${c} ${typeof row[i] === "number" && !Number.isInteger(row[i]) ? row[i].toFixed(3) : row[i]}`))));
        await loadAux(); setTimeout(draw, 1500);
      } catch (e) { clear(report, failure(e, "Training")); } finally { train.disabled = false; }
    } }, "Train");
    gate(train, "admin");
    show();
    return card({ title: "Classifiers", sub: "CREATE CLASSIFIER", body: h("div", { class: "qu-col" }, listEl,
      h("div", { class: "row" }, h("span", { class: "small dim" }, "Name"), name, h("span", { class: "small dim" }, "learn"), lab, h("span", { class: "small dim" }, "from"), vec, method),
      where, pre, h("div", { class: "row" }, train), report) });
  }

  function derivedCard(T) {
    const d = derivedKeywords(T);
    return card({ title: "Derived keywords", sub: "OPTIONS (derive = ..., model = ...)", body: h("div", { class: "qu-col" },
      d.length ? h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, h("th", {}, "field"), h("th", {}, "model"), h("th", {}, "of"))),
        h("tbody", {}, d.map((f) => h("tr", {}, h("td", { class: "mono" }, f.name + (f.multi ? "[]" : "")), h("td", {}, f.derive.model), h("td", {}, [].concat(f.derive.from).join(", ")))))) : h("div", { class: "small dim" }, "None on this table."),
      h("div", { class: "small dim" }, "A KEYWORD field made of a text when a row is written: builtin:lang, builtin:keyphrases, builtin:minhash, dict:<table.field> or classifier:<name>. Preview rows shows what each holds."),
      sqlBlock(`ALTER TABLE ${quoteName(st.table)} ADD COLUMN lang KEYWORD OPTIONS (derive = '${st.field}', model = 'builtin:lang')`)) });
  }

  function synonymCard(T) {
    const own = tableList();
    const area = h("textarea", { class: "input mono qu-area", spellcheck: false, placeholder: "sofa, couch\ntv => television" }, own?.text || "");
    const status = h("div", { class: "small dim" }, own ? `${own.rules} rules, version ${own.version}` : "No list of its own yet.", synonyms?.rules ? ` The server's list has ${synonyms.rules} rules; a search with synonyms => 1 uses both.` : "");
    const saveBtn = h("button", { class: "btn primary sm", onclick: async () => {
      const text = area.value.trim();
      if (!text) return toast("The list is empty: Remove drops it", "warn");
      try { await sql(`CREATE ${own ? "OR REPLACE " : ""}SYNONYMS ON ${quoteName(st.table)} AS ${lit(text)}`, "Saved the synonym list"); await loadAux(); draw(); } catch { /* toasted */ }
    } }, own ? "Save as the next version" : "Save");
    const dropBtn = own ? h("button", { class: "btn sm danger", onclick: async () => { if (await confirm({ title: `Remove ${st.table}'s synonym list?`, body: "Searches with synonyms => 1 then use the server's list alone.", ok: "Remove", danger: true })) { await sql(`DROP SYNONYMS ON ${quoteName(st.table)}`, "Removed the list"); await loadAux(); draw(); } } }, "Remove") : null;
    gate(saveBtn, "admin"); if (dropBtn) gate(dropBtn, "admin");

    // Mining.
    const m = st.mine || (st.mine = { source: st.vector ? "vectors" : "log", log: "", min_cos: 0.6, per_word: 3 });
    const proposals = h("div", { class: "qu-col" });
    const MF = storedField();
    const vecOk = !!st.vector && !!MF;
    const logSel = h("input", { class: "input sm", placeholder: "table of searches (context, ts, text)", value: m.log, oninput: () => { m.log = logSel.value.trim(); save(); } });
    const cos = h("input", { class: "input sm", type: "number", step: "0.05", min: -1, max: 1, value: m.min_cos, style: { width: "70px" }, oninput: () => { m.min_cos = +cos.value; save(); } });
    const src = seg([["vectors", "from vectors"], ["log", "from a search log"]], m.source, (v) => { m.source = v; save(); draw(); });
    const mineBtn = h("button", { class: "btn sm", onclick: async () => {
      mineBtn.disabled = true; clear(proposals, h("div", { class: "small dim" }, "Mining…"));
      const b = { table: st.table, field: m.source === "vectors" ? MF : st.field, ...(m.source === "vectors" ? { vector: st.vector, min_cos: m.min_cos, per_word: m.per_word } : { log: m.log }) };
      try { showProposals(await api("/v1/synonyms/mine", { body: b })); } catch (e) { clear(proposals, failure(e, "Mining")); } finally { mineBtn.disabled = false; }
    } }, "Mine");
    if (m.source === "vectors" && !vecOk) mineBtn.disabled = true;
    function showProposals(j) {
      const picked = new Set(j.proposals.map((_, i) => i));
      const rows = j.proposals.map((p, i) => h("tr", {},
        h("td", {}, h("input", { type: "checkbox", checked: true, onchange: (e) => { if (e.target.checked) picked.add(i); else picked.delete(i); } })),
        h("td", { class: "mono" }, p.rule), h("td", { class: "num" }, m.source === "vectors" ? p.score.toFixed(3) : String(p.score)), h("td", { class: "num" }, `${p.from_count} / ${p.to_count}`)));
      const add = h("button", { class: "btn sm", onclick: () => { const lines = j.proposals.filter((_, i) => picked.has(i)).map((p) => p.rule); area.value = (area.value.trim() ? area.value.trim() + "\n" : "") + lines.join("\n"); toast(`${lines.length} rules added to the list: Save to keep them`, "info"); } }, "Add the ticked rules to the list");
      clear(proposals,
        h("div", { class: "small dim" }, `${j.proposals.length} proposals from ${fmt.n ? fmt.n(j.words) : j.words} words of ${fmt.n ? fmt.n(j.rows) : j.rows} ${j.source === "log" ? "searches" : "rows"}, in ${Math.round(j.took_ms)} ms. Close vectors also join words that are related but not the same (colours, sizes): keep what means the same.`),
        j.proposals.length ? h("div", { class: "tbl-wrap", style: { maxHeight: "340px" } }, h("table", { class: "t compact" }, h("thead", {}, h("tr", {}, h("th", {}, ""), h("th", {}, "rule"), h("th", {}, m.source === "vectors" ? "cosine" : "contexts"), h("th", {}, "counts"))), h("tbody", {}, rows))) : null,
        j.proposals.length ? add : null);
    }
    return card({ title: "Synonyms", sub: "CREATE SYNONYMS ON table", body: h("div", { class: "qu-col" },
      status, area, h("div", { class: "row" }, saveBtn, dropBtn),
      h("div", { class: "qu-sep" }),
      h("div", { class: "row" }, h("b", { class: "small" }, "Mine proposals"), src),
      m.source === "vectors"
        ? h("div", { class: "row" }, h("span", { class: "small dim" }, vecOk ? `words of ${MF}, embedded by ${st.vector}'s model; cosine at least` : "Mining from vectors needs a stored text field and a vector field with a model."), vecOk ? cos : null, h("div", { class: "qu-spacer" }), mineBtn)
        : h("div", { class: "row" }, logSel, h("div", { class: "qu-spacer" }), mineBtn),
      proposals) });
  }
}

// Query templates: a search designed stage by stage (views/designer.js),
// whose one input is the text a person typed. A template is a plain
// object (its design) that this module turns into SQL: the query the
// designer previews, and the engine objects it is saved as.
//
// The stages, in the order a query meets them (chloedb::query::funnel):
//
//   understanding  words that name a value of the table (a colour, a
//                  brand, "under 200") become a boost or a filter
//                  (`understand =>`)
//   retrieval      the candidates: words (BM25 over one or more fields,
//                  with typos, synonyms and terms from the first rows,
//                  RM3's `expand =>`), meaning (`similarity` of a vector
//                  field the server embeds the text for), fixed filters,
//                  fused by `rrf` or a weighted sum, cut to N candidates
//   ranking        the candidates ranked again: boosts of the retrieval's
//                  score (by a number, by recency, by a condition), then
//                  heads over its top N, each a prediction with a weight
//                  (words, meaning by any embedding, a number, recency, a
//                  learned model `gbdt`, a cross-encoder), fused by
//                  weights over `minmax` or `zscore`, or by rank (`rrf`),
//                  in a second phase
//   blending       the page from several queries: the template's own
//                  rows, other saved templates, words over another table,
//                  any query; diversity, a cap per value, near duplicates
//                  dropped, a quota of a kind (`CREATE BLEND`)
//
// Saved, a template named `t` is four engine objects, each a CREATE
// statement, each versioned and shipped to replicas and shards as the
// engine ships them:
//
//   CREATE OR REPLACE RANK PROFILE t ON table ...   the ranking (the
//       retrieval's first phase and the ranking's phases), its boost
//       weights its parameters, so the relevance autopilot can tune it
//   CREATE OR REPLACE BLEND t_blend ...             the page, when on
//   CREATE OR REPLACE FUNCTION t(q TEXT, k INT DEFAULT 10) RETURNS TABLE
//       what apps call: SELECT * FROM t('red shoes'); agents call it as
//       the MCP tool fn_t
//   CREATE OR REPLACE FUNCTION t_template() RETURNS TEXT AS 'template:{...}'
//       the design itself, so the designer opens it again (a blend keeps
//       its definition the same way)

import { quoteName } from "./sqledit.js";

/** How a template's design function's body begins. */
export const MARK = "template:";
/** The design's version: a later one adds fields, with defaults. */
export const VERSION = 1;

export const lit = (v) => `'${String(v).replace(/'/g, "''")}'`;
const q = (n) => quoteName(n);
const num = (v, what = "a number") => { const x = Number(v); if (v === "" || v == null || !Number.isFinite(x)) throw new Error(`${what} needs a number`); return String(x); };
const r3 = (x) => String(Math.round(Number(x) * 1000) / 1000);
const ident = (s) => String(s || "").toLowerCase().replace(/[^a-z0-9_]+/g, "_").replace(/^_+|_+$/g, "").replace(/^(\d)/, "_$1") || "x";

/** A template's name, as SQL names a function: lower case letters, digits and _. */
export const slug = (s) => String(s || "").toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").replace(/^(\d)/, "t_$1").slice(0, 40);

// ------------------------------------------------------------ fields

export const fieldsOf = (t) => t?.short?.fields || [];
export const textFields = (t) => fieldsOf(t).filter((f) => f.type === "text" && f.indexed !== false);
export const vectorFields = (t) => fieldsOf(t).filter((f) => f.type === "vector" && !f.multi);
export const numberFields = (t) => fieldsOf(t).filter((f) => (f.type === "int" || f.type === "float") && !f.multi);
export const timeFields = (t) => fieldsOf(t).filter((f) => f.type === "timestamp" && !f.multi);
export const keywordFields = (t) => fieldsOf(t).filter((f) => f.type === "keyword" && f.name !== t?.short?.key);
const isStored = (f) => f.stored != null ? !!f.stored : f.type === "vector" ? false : f.type === "text" ? !(f.from && f.from.length) : true;
export const showable = (t) => fieldsOf(t).filter((f) => f.type !== "vector" && f.type !== "sparse" && isStored(f));

/** The text field a search reads first: a title or name, else a body, else the first. */
function mainText(t) {
  const ts = textFields(t);
  const by = (re) => ts.find((f) => re.test(f.name));
  return by(/^(title|name|subject|headline)$/i) || by(/^(body|content|text|description|summary|abstract)$/i) || ts[0] || null;
}

/** What a row is shown by, as a result: its key, its title, a line of text, and a few facts (a brand, a price, a rating, a date). */
function displayColumns(t) {
  const key = t?.short?.key;
  const shown = showable(t).filter((f) => f.name !== key && !f.multi);
  const text = shown.filter((f) => f.type === "text");
  const title = text.find((f) => /^(title|name|subject|headline)$/i.test(f.name)) || text[0];
  const line = text.find((f) => f !== title && /^(description|summary|abstract|snippet|body|content|text)$/i.test(f.name));
  const facts = [
    shown.find((f) => f.type === "keyword" && /brand|author|source|site|by|maker|vendor/i.test(f.name)),
    shown.find((f) => f.type === "keyword" && /categor|type|kind|section|topic/i.test(f.name)),
    shown.find((f) => (f.type === "float" || f.type === "int") && /price|cost|amount/i.test(f.name)),
    shown.find((f) => (f.type === "float" || f.type === "int") && /rating|stars|score|points/i.test(f.name)),
    shown.find((f) => f.type === "timestamp") || shown.find((f) => f.type === "int" && /^year$/i.test(f.name)),
  ].filter(Boolean);
  return [key, title?.name, line?.name, ...facts.map((f) => f.name)].filter(Boolean);
}

// ------------------------------------------------------------ the design

/** A new template's design over table `t` (the catalog's entry), with what the table suggests on. */
export function blank(t, name = "") {
  const main = mainText(t);
  const others = textFields(t).filter((f) => f !== main && /^(body|content|text|description|summary|abstract)$/i.test(f.name)).slice(0, 1);
  const vec = vectorFields(t).find((f) => f.embed) || null;
  return {
    v: VERSION,
    name: slug(name),
    table: t?.table || "",
    about: "",
    samples: [],
    understand: { values: false, mode: "boost", fields: [], weight: 0.25 },
    words: {
      on: !!main,
      fields: [main, ...others].filter(Boolean).map((f, i) => ({ field: f.name, weight: i ? 0.5 : 1 })),
      match: "any",
      typos: "auto",
      synonyms: false,
      feedback: 0,
      plain: true,
    },
    meaning: { on: false, field: vec?.name || "", weight: 1 },
    fusion: "rrf",
    filters: [],
    candidates: 100,
    rank: { boosts: [], heads: { on: false, depth: 100, fuse: "weighted", norm: "minmax", list: [] }, rerank: { on: false, model: "", fields: [], depth: 30 } },
    blend: { on: false, diversify: null, cap: null, dedupe: null, quota: null, sources: [], depth: 50 },
    output: { columns: displayColumns(t), k: 10 },
  };
}

/** A design read back (a saved one, or an older version), with every field it lacks at its default. */
export function normalize(d, t) {
  const b = blank(t, d?.name);
  const out = { ...b, ...d, v: VERSION };
  for (const k of ["understand", "words", "meaning", "output"]) out[k] = { ...b[k], ...(d?.[k] || {}) };
  out.rank = { ...b.rank, ...(d?.rank || {}), rerank: { ...b.rank.rerank, ...(d?.rank?.rerank || {}) } };
  out.rank.boosts = Array.isArray(out.rank.boosts) ? out.rank.boosts : [];
  out.rank.heads = { ...b.rank.heads, ...(d?.rank?.heads || {}) };
  out.rank.heads.list = Array.isArray(out.rank.heads.list) ? out.rank.heads.list : [];
  // A rerank of version 1 is a cross-encoder head of the top rows.
  if (out.rank.rerank.on && out.rank.rerank.model && !out.rank.heads.list.length) {
    const rr = out.rank.rerank;
    out.rank.heads = { ...out.rank.heads, on: true, depth: rr.depth || 20, list: [{ kind: "cross", model: rr.model, fields: rr.fields || [], w: 1 }] };
  }
  out.rank.rerank = { ...out.rank.rerank, on: false };
  out.blend = { ...b.blend, ...(d?.blend || {}) };
  out.blend.sources = (Array.isArray(out.blend.sources) ? out.blend.sources : []).map((x) => ({ kind: "table", ...x }));
  out.filters = Array.isArray(out.filters) ? out.filters : [];
  out.samples = Array.isArray(out.samples) ? out.samples : [];
  out.words.fields = Array.isArray(out.words.fields) ? out.words.fields : [];
  out.understand.fields = Array.isArray(out.understand.fields) ? out.understand.fields : String(out.understand.fields || "").split(",").map((x) => x.trim()).filter(Boolean);
  return out;
}

/** The design a stored function's body holds, if it is a template's ('template:{...}'). */
export function ofBody(body) {
  const s = String(body || "").trim();
  if (!s.startsWith("'") || !s.endsWith("'")) return null;
  const inner = s.slice(1, -1).replace(/''/g, "'");
  if (!inner.startsWith(MARK)) return null;
  try { return JSON.parse(inner.slice(MARK.length)); } catch { return null; }
}

// ------------------------------------------------------------ SQL

/** A filter's condition, as SQL. */
export function condition(f, c) {
  const n = q(c.field);
  const isNum = f && (f.type === "int" || f.type === "float");
  const val = (v) => (isNum ? num(v, c.field) : lit(v ?? ""));
  switch (c.op) {
    case "in": { const vs = String(c.value || "").split(",").map((x) => x.trim()).filter(Boolean); if (!vs.length) throw new Error(`${c.field} needs values`); return `${n} IN (${vs.map(val).join(", ")})`; }
    case "null": return `${n} IS NULL`;
    case "notnull": return `${n} IS NOT NULL`;
    case "true": return `${n} = TRUE`;
    case "false": return `${n} = FALSE`;
    case "matches": return `MATCH(${n}, ${lit(c.value || "")})`;
    case "between": return `${n} BETWEEN ${val(c.value)} AND ${val(c.value2)}`;
    case "prefix": return `PREFIX(${n}, ${lit(c.value || "")})`;
    default: return `${n} ${["=", "!=", "<", "<=", ">", ">="].includes(c.op) ? c.op : "="} ${val(c.value)}`;
  }
}

/** The options of a words field's bm25 (and of its MATCH), as `name => value`. */
function wordOptions(d, primary, { match = false } = {}) {
  const w = d.words, o = [];
  const feedback = Number(w.feedback) > 0 && !match;
  if (w.plain && !feedback) o.push("syntax => 'plain'");
  if (w.typos && w.typos !== "off") o.push(`fuzzy => ${w.typos === "auto" ? "'auto'" : num(w.typos)}`);
  if (w.synonyms) o.push("synonyms => 1");
  if (feedback) o.push(`expand => ${Math.min(100, Math.round(Number(w.feedback)))}`);
  const u = d.understand;
  if (!match && primary && u.values && u.fields.length) {
    o.push(`understand => '${u.mode === "filter" ? "filter" : "boost"}'`, `understand_fields => ${lit(u.fields.join(", "))}`);
    if (u.weight != null && Number(u.weight) !== 0.25) o.push(`understand_weight => ${r3(u.weight)}`);
  }
  return o;
}

/** The retrieval's inputs: [{kind, label, sql, weight}]. */
export function inputs(d, t) {
  const out = [];
  const has = (n) => fieldsOf(t).some((f) => f.name === n);
  if (d.words.on) {
    d.words.fields.filter((x) => has(x.field)).forEach((x, i) => {
      const o = wordOptions(d, i === 0);
      out.push({ kind: "words", field: x.field, label: `words in ${x.field}`, sql: `bm25(${q(x.field)}, :q${o.length ? ", " + o.join(", ") : ""})`, weight: Number(x.weight ?? 1) });
    });
  }
  if (d.meaning.on && d.meaning.field && has(d.meaning.field)) out.push({ kind: "meaning", field: d.meaning.field, label: `meaning of ${d.meaning.field}`, sql: `similarity(${q(d.meaning.field)}, :q)`, weight: Number(d.meaning.weight ?? 1) });
  return out;
}

/**
 * The retrieval's score: one input, or the inputs fused. Fused, each input
 * gives its top `candidates` rows (`depth =>`); one text score alone ranks
 * every row it matches exactly (a rank-safe boost keeps that).
 */
export function firstScore(d, t) {
  const ins = inputs(d, t);
  if (!ins.length) throw new Error("Retrieval needs words or meaning: turn one on");
  if (ins.length === 1) {
    const one = ins[0];
    // All the words while enough rows have them, else fewer (bm25's guard).
    if (one.kind === "words" && d.words.match === "relax" && !(Number(d.words.feedback) > 0)) return one.sql.replace(/\)$/, `, min => 1, min_rows => ${Math.max(1, Math.round(Number(d.output.k) || 10))})`);
    return one.sql;
  }
  const depth = Math.max(1, Math.min(10000, Math.round(Number(d.candidates) || 100)));
  const deep = depth !== 100;
  if (d.fusion === "weighted") {
    const total = ins.reduce((a, x) => a + (x.weight || 0), 0) || 1;
    return ins.map((x) => `${r3((x.weight || 0) / total)} * tmm(${deep ? x.sql.replace(/\)$/, `, depth => ${depth})`) : x.sql})`).join(" + ");
  }
  const ws = ins.map((x) => x.weight ?? 1);
  return `rrf(${ins.map((x) => x.sql).join(", ")}${ws.some((w) => w !== 1) ? `, weights => [${ws.map(r3).join(", ")}]` : ""}${deep ? `, depth => ${depth}` : ""})`;
}

/** The ranking's boosts: [{sql, param, value, label}], each weight a parameter. */
export function boosts(d, t) {
  const out = [];
  const F = (n) => fieldsOf(t).find((f) => f.name === n);
  d.rank.boosts.forEach((b, i) => {
    const f = F(b.field);
    if (!f) return;
    const p = ident(`w_${b.field}${d.rank.boosts.slice(0, i).some((x) => x.field === b.field) ? "_" + i : ""}`);
    const w = Number(b.w ?? 0.1);
    if (b.kind === "recency") {
      const scale = Math.max(1, Math.round(Number(b.days || 30) * 86400));
      out.push({ param: p, value: w, label: `newer ${b.field}`, sql: `(1 + :${p} * decay(${q(b.field)}, now(), ${scale}))` });
    } else if (b.kind === "closeness") {
      out.push({ param: p, value: w, label: `${b.field} near ${b.origin}`, sql: `(1 + :${p} * decay(${q(b.field)}, ${num(b.origin, b.field)}, ${num(b.scale || 1, b.field)}))` });
    } else if (b.kind === "condition") {
      out.push({ param: p, value: w, label: `${b.field} ${b.op || "="} ${b.value ?? ""}`, sql: `(1 + :${p} * CASE WHEN ${condition(f, { field: b.field, op: b.op || "=", value: b.value })} THEN 1 ELSE 0 END)` });
    } else {
      const v = `coalesce(${q(b.field)}, 0)`;
      out.push({ param: p, value: w, label: `more ${b.field}`, sql: b.log ? `(1 + :${p} * ln(1 + ${v}))` : `(1 + :${p} * ${v})` });
    }
  });
  return out;
}

/** The kinds of a ranking head: [kind, label, what it predicts]. */
export const HEAD_KINDS = [
  ["words", "Words", "BM25 of the query's words in a text field"],
  ["meaning", "Meaning", "similarity of an embedding to the query's, by the field's model"],
  ["number", "A number", "a column of the row: rating, sales, clicks"],
  ["recency", "Recency", "how new the row is, halving every so many days"],
  ["model", "A learned model", "a LightGBM or XGBoost model (gbdt) over features of the row"],
  ["cross", "A cross-encoder", "a model that reads the query and the row together (or a language model that grades it)"],
];
const FEATURE_KINDS = ["words", "meaning", "number", "recency"];

/** A head's (or a model's feature's) score, as SQL. */
export function headSql(x, d, t) {
  const f = fieldsOf(t).find((y) => y.name === x.field);
  const need = (ok, what) => { if (!ok) throw new Error(what); };
  switch (x.kind) {
    case "words": need(f && f.type === "text", "A words head needs a text field"); return `bm25(${q(f.name)}, :q${d.words.plain ? ", syntax => 'plain'" : ""})`;
    case "meaning": need(f && f.type === "vector", "A meaning head needs a vector field"); return `similarity(${q(f.name)}, :q)`;
    case "number": need(f, "A number head needs a number field"); return x.log ? `ln(1 + coalesce(${q(f.name)}, 0))` : `coalesce(${q(f.name)}, 0)`;
    case "recency": need(f, "A recency head needs a time field"); return `decay(${q(f.name)}, now(), ${Math.max(1, Math.round(Number(x.days || 30) * 86400))})`;
    case "model": {
      need(x.model, "A learned model head needs the model's name");
      const fs = (x.features || []).filter((y) => FEATURE_KINDS.includes(y.kind));
      need(fs.length, `${x.model} needs its features, in the order it was trained on`);
      const g = `gbdt(${lit(x.model)}, ${fs.map((y) => headSql(y, d, t)).join(", ")})`;
      return x.prob ? `sigmoid(${g})` : g;
    }
    case "cross": {
      need(x.model, "A cross-encoder head needs the model's name");
      // Unpicked, the text it reads is the stored text fields: the words
      // fields, else what the rows show (a field made of others is not kept).
      const stored = (n) => fieldsOf(t).some((y) => y.name === n && (y.type === "text" || y.type === "keyword") && isStored(y));
      const words = d.words.fields.map((y) => y.field).filter(stored);
      const fs = x.fields?.length ? x.fields.filter((n) => fieldsOf(t).some((y) => y.name === n)) : words.length ? words : showable(t).filter((y) => y.type === "text").map((y) => y.name);
      need(fs.length, "A cross-encoder needs a stored text field to read");
      return `cross_encoder(${lit(x.model)}, ${fs.map(q).join(", ")}, :q)`;
    }
    default: throw new Error(`No head of kind ${x.kind}`);
  }
}

/** A head's name, for people. */
export function headLabel(x) {
  switch (x.kind) {
    case "words": return `words in ${x.field}`;
    case "meaning": return `meaning of ${x.field}`;
    case "number": return x.log ? `log of ${x.field}` : x.field;
    case "recency": return `newer ${x.field}`;
    case "model": return `${x.prob ? "probability of " : ""}${x.model || "a model"}`;
    case "cross": return x.model || "a cross-encoder";
    default: return x.kind;
  }
}

/** The ranking's heads, when on: [{sql, label, w, kind}]. */
export function heads(d, t) {
  const hd = d.rank.heads;
  if (!hd?.on) return [];
  return hd.list.map((x) => ({ sql: headSql(x, d, t), label: headLabel(x), w: Number(x.w ?? 1), kind: x.kind }));
}

/** The heads fused into one score: one head alone, a weighted sum over `minmax` or `zscore`, or `rrf`. */
export function headsScore(d, t) {
  const hs = heads(d, t);
  if (!hs.length) return null;
  if (hs.length === 1) return hs[0].sql;
  const hd = d.rank.heads;
  if (hd.fuse === "rrf") {
    const ws = hs.map((x) => x.w);
    return `rrf(${hs.map((x) => x.sql).join(", ")}${ws.some((w) => w !== 1) ? `, weights => [${ws.map(r3).join(", ")}]` : ""})`;
  }
  const norm = hd.norm === "zscore" ? "zscore" : "minmax";
  return hs.map((x, i) => `${i && x.w >= 0 ? "+ " : i ? "- " : x.w < 0 ? "-" : ""}${r3(Math.abs(x.w))} * ${norm}(${x.sql})`).join(" ");
}

/** How many rows the heads read: the first phase's top N. */
export const headsDepth = (d) => Math.max(1, Math.min(10000, Math.round(Number(d.rank.heads?.depth) || 100)));

/** The fixed filters, as WHERE's conditions. */
export function where(d, t) {
  const out = [];
  const F = (n) => fieldsOf(t).find((f) => f.name === n);
  // All the words: a MATCH of the first words field.
  if (d.words.on && d.words.match === "all") {
    const first = d.words.fields.find((x) => F(x.field));
    if (first) { const o = wordOptions(d, true, { match: true }); out.push(`MATCH(${q(first.field)}, :q${o.length ? ", " + o.join(", ") : ""})`); }
  }
  for (const c of d.filters) { const f = F(c.field); if (f) out.push(condition(f, c)); }
  return out;
}

/**
 * The ranking's phases: [{name, stage, by, limit}] in order. The first is
 * the retrieval's score times the boosts (a product the engine ranks
 * exactly for one text score); a rerank reads its top rows again. A later
 * phase never repeats the first's text scores: a server rewrites those
 * (understood words, expanded terms) by the table they read.
 */
export function phases(d, t) {
  const first = firstScore(d, t);
  const bs = boosts(d, t);
  const rr = d.rank.rerank;
  const base = bs.length && / \+ /.test(first) ? `(${first})` : first;
  const out = [{ name: "ranked", stage: bs.length ? "ranking" : "retrieval", by: [base, ...bs.map((b) => b.sql)].join(" * "), limit: null }];
  const hs = headsScore(d, t);
  if (hs) {
    out[0].name = "candidates";
    out[0].limit = headsDepth(d);
    out.push({ name: "ranked", stage: "ranking", by: hs, limit: null });
  }
  if (rr.on && rr.model) {
    out[out.length - 1].limit = Math.max(1, Math.min(1000, Math.round(Number(rr.depth) || 30)));
    const fs = (rr.fields?.length ? rr.fields : d.words.fields.map((x) => x.field)).filter((n) => fieldsOf(t).some((f) => f.name === n));
    if (!fs.length) throw new Error("The cross-encoder needs a text field to read");
    out.push({ name: "reranked", stage: "ranking", by: `cross_encoder(${lit(rr.model)}, ${fs.map(q).join(", ")}, :q)`, limit: null });
  }
  return out;
}

/** The parameters a preview passes: the text, and each boost's weight. */
export function params(d, t, text) {
  return { q: text, ...Object.fromEntries(boosts(d, t).map((b) => [b.param, b.value])) };
}

/** The columns a query of the template answers, besides score(). */
export function columns(d, t) {
  const have = new Set(fieldsOf(t).map((f) => f.name));
  const cs = (d.output.columns || []).filter((c) => have.has(c));
  return cs.length ? cs.map(q) : ["*"];
}

/**
 * The query, written out: its phases as WITH steps, `where` in the first,
 * the page's LIMIT on the last. `upTo`: stop after that stage
 * ("retrieval"), so a step's preview shows the rows as they leave it.
 * `extra` columns are added to the answer (a blend's source needs its
 * rules' columns).
 */
export function querySql(d, t, { upTo = null, limit = null, extra = [], select = null } = {}) {
  let ps = phases(d, t);
  if (upTo === "retrieval") ps = [{ name: "ranked", by: firstScore(d, t), limit: null }];
  const ws = where(d, t);
  const k = Math.max(1, Math.round(Number(limit ?? d.output.k) || 10));
  const cols = select || [...columns(d, t), ...extra.filter((c) => !columns(d, t).includes(q(c))).map(q)];
  const proj = cols.includes("*") && cols.length > 1 ? ["*"] : cols;
  const T = q(t.table);
  if (ps.length === 1) return `SELECT ${[...proj, "score()"].join(", ")}\nFROM ${T}${ws.length ? `\nWHERE ${ws.join("\n  AND ")}` : ""}\nORDER BY ${ps[0].by} DESC\nLIMIT ${k}`;
  const steps = ps.slice(0, -1).map((p, i) => `${p.name} AS (SELECT * FROM ${i ? ps[i - 1].name : T}${!i && ws.length ? ` WHERE ${ws.join(" AND ")}` : ""} ORDER BY ${p.by} DESC LIMIT ${p.limit})`);
  const last = ps[ps.length - 1];
  return `WITH ${steps.join(",\n     ")}\nSELECT ${[...proj, "score()"].join(", ")} FROM ${ps[ps.length - 2].name}\nORDER BY ${last.by} DESC\nLIMIT ${k}`;
}

/** The rank profile's ranking: the phases over the whole table, with no WHERE and no LIMIT on the last. */
export function profileSql(d, t) {
  const ps = phases(d, t);
  const T = q(t.table);
  const bs = boosts(d, t);
  const defaults = bs.length ? `\nWITH (${bs.map((b) => `${b.param} => ${r3(b.value)}`).join(", ")})` : "";
  let body;
  if (ps.length === 1) body = `SELECT * FROM ${T} ORDER BY ${ps[0].by} DESC`;
  else {
    const steps = ps.slice(0, -1).map((p, i) => `${p.name} AS (SELECT * FROM ${i ? ps[i - 1].name : T} ORDER BY ${p.by} DESC LIMIT ${p.limit})`);
    body = `WITH ${steps.join(",\n       ")}\n  SELECT * FROM ${ps[ps.length - 2].name} ORDER BY ${ps[ps.length - 1].by} DESC`;
  }
  return `CREATE OR REPLACE RANK PROFILE ${d.name} ON ${T} AS\n  ${body}${defaults}`;
}

/** A query of the template's table ranked by its saved profile: the template function's body. */
export function rankedSql(d, t, { limit = ":k", extra = [], alias = null } = {}) {
  const ws = where(d, t);
  const cs = [...columns(d, t), ...extra.filter((c) => !columns(d, t).includes(q(c))).map(q)];
  const proj = cs.includes("*") && cs.length > 1 ? ["*"] : cs;
  return `SELECT ${[...proj, alias ? `score() AS ${alias}` : "score()"].join(", ")} FROM ${q(t.table)}${ws.length ? ` WHERE ${ws.join(" AND ")}` : ""} ORDER BY rank_profile(${lit(d.name)}, q => :q) LIMIT ${limit}`;
}

/** The columns the blend's rules read. */
function blendColumns(d) {
  const b = d.blend;
  return [b.diversify?.on, b.cap?.per, b.dedupe?.on, b.quota?.column].filter(Boolean);
}

/**
 * The blend's definition (chloedb::query::blend::Def), with the
 * template's own results as the source `results` (written out for a
 * preview, by the profile when saved) and the other sources' queries.
 */
export function blendDef(d, t, { saved = false } = {}) {
  const b = d.blend;
  const depth = Math.max(1, Math.round(Number(b.depth) || 50));
  const extra = blendColumns(d);
  const main = saved ? rankedSql(d, t, { limit: depth, extra }) : querySql(d, t, { limit: depth, extra });
  const def = { name: `${d.name || "draft"}_blend`, sources: [{ name: "results", sql: main }], k: Math.max(1, Math.round(Number(d.output.k) || 10)) };
  const boostsBy = [];
  if (Number(b.boost)) boostsBy.push({ source: "results", by: Number(b.boost) });
  const taken = new Set(["results"]);
  for (const s of b.sources || []) {
    const sql = sourceSql(s);
    if (!sql) continue;
    let name = ident(s.name || s.template || s.table || "query");
    while (taken.has(name)) name += "_2";
    taken.add(name);
    def.sources.push({ name, sql });
    if (Number(s.boost)) boostsBy.push({ source: name, by: Number(s.boost) });
  }
  if (boostsBy.length) def.boost = boostsBy;
  if (b.diversify?.on) def.diversify = { lambda: Number(b.diversify.lambda ?? 0.3), on: b.diversify.on };
  if (b.cap?.per) def.caps = [{ at_most: Math.max(1, Math.round(Number(b.cap.at_most) || 2)), per: b.cap.per }];
  if (b.dedupe?.on) def.dedupe = { threshold: Number(b.dedupe.threshold ?? 0.95), on: b.dedupe.on };
  if (b.quota?.column && b.quota.value != null && b.quota.value !== "") {
    def.quotas = [{ when: { column: b.quota.column, value: b.quota.value }, min: Number(b.quota.min ?? 0), max: Number(b.quota.max ?? 100), ...(b.quota.top ? { top: Math.round(Number(b.quota.top)) } : {}) }];
  }
  if (d.about) def.note = `query template ${d.name}`;
  return def;
}

/** The kinds of a blend's other sources: [kind, label, what it is]. */
export const SOURCE_KINDS = [
  ["template", "A saved template", "another template's rows for the same text, ranked its way"],
  ["table", "Words in a table", "the rows of a table whose text holds the words"],
  ["sql", "A query", "any query of its own, :q the text"],
];

/** An other source's query, or null while it is not filled in. */
export function sourceSql(s) {
  const n = Math.max(1, Math.round(Number(s.depth) || 20));
  switch (s.kind || "table") {
    case "template": return s.template ? `SELECT * FROM ${q(s.template)}(q => :q, k => ${n})` : null;
    case "sql": return String(s.sql || "").trim().replace(/;\s*$/, "") || null;
    default: return s.table && s.field ? `SELECT *, score() FROM ${q(s.table)} ORDER BY bm25(${q(s.field)}, :q, syntax => 'plain') DESC LIMIT ${n}` : null;
  }
}

/** How a blend's definition is written as CREATE BLEND (chloedb::query::blend::Def::sql). */
export function blendSql(def) {
  const pred = (p) => `${q(p.column)} = ${typeof p.value === "number" ? p.value : lit(p.value)}`;
  const parts = [`CREATE OR REPLACE BLEND ${def.name} AS\n  SOURCES (\n${def.sources.map((s) => `    ${s.name} AS (${s.sql})`).join(",\n")}\n  )`];
  if (def.boost?.length) parts.push(`  BOOST (${def.boost.map((b) => `${b.source} ${b.by}`).join(", ")})`);
  for (const x of def.quotas || []) {
    let s = pred(x.when);
    if (x.min > 0 && x.max < 100) s += ` BETWEEN ${x.min}% AND ${x.max}%`; else if (x.min > 0) s += ` AT LEAST ${x.min}%`; else s += ` AT MOST ${x.max}%`;
    if (x.top) s += ` IN TOP ${x.top}`;
    parts.push(`  QUOTA (${s})`);
  }
  if (def.diversify) parts.push(`  DIVERSIFY ${def.diversify.lambda} ON ${q(def.diversify.on)}`);
  for (const c of def.caps || []) parts.push(`  MAX ${c.at_most} PER ${q(c.per)}`);
  if (def.dedupe) parts.push(`  DEDUPE ${def.dedupe.threshold} ON ${q(def.dedupe.on)}`);
  parts.push(`  WITH (k => ${def.k}${def.note ? `, note => ${lit(def.note)}` : ""})`);
  return parts.join("\n");
}

/** What the template's function says it finds, for people and agents. */
export function describe(d, t) {
  if (d.about?.trim()) return d.about.trim();
  const ins = inputs(d, t).map((x) => x.label);
  let hs = [];
  try { hs = heads(d, t); } catch { /* the editor says why */ }
  return `Searches ${t.table} by ${ins.join(" and ") || "nothing yet"}${d.rank.boosts.length ? ", boosted by " + boosts(d, t).map((b) => b.label).join(", ") : ""}${hs.length ? ", ranked by " + hs.map((x) => x.label).join(", ") : ""}${d.blend.on ? `, blended${d.blend.sources.length ? ` with ${d.blend.sources.length} other ${d.blend.sources.length === 1 ? "query" : "queries"}` : ""}` : ""}.`;
}

/**
 * The statements that save a template, in order: [{what, sql}]. `had`:
 * whether a blend of this template exists now (dropped when blending is off).
 */
export function saveStatements(d, t, { hadBlend = false } = {}) {
  if (!d.name) throw new Error("Name the template");
  if (!/^[a-z_][a-z0-9_]*$/.test(d.name)) throw new Error("A template's name is lower case letters, digits and _");
  const out = [{ what: "ranking", sql: profileSql(d, t) }];
  const blendName = `${d.name}_blend`;
  let body;
  if (d.blend.on) {
    out.push({ what: "blend", sql: blendSql(blendDef(d, t, { saved: true })) });
    body = `SELECT * FROM blend(${lit(blendName)}, q => :q) LIMIT :k`;
  } else {
    if (hadBlend) out.push({ what: "blend", sql: `DROP BLEND IF EXISTS ${blendName}` });
    body = rankedSql(d, t);
  }
  out.push({ what: "template", sql: `CREATE OR REPLACE FUNCTION ${d.name}(q TEXT, k INT DEFAULT ${Math.max(1, Math.round(Number(d.output.k) || 10))}) RETURNS TABLE OPTIONS (description = ${lit(describe(d, t))}) AS\n  ${body}` });
  const { savedVersion: _v, hadBlend: _b, ...kept } = d;
  const design = JSON.stringify({ ...kept, v: VERSION });
  out.push({ what: "design", sql: `CREATE OR REPLACE FUNCTION ${d.name}_template() RETURNS TEXT OPTIONS (description = ${lit(`the design of query template ${d.name}`)}) AS ${lit(MARK + design)}` });
  return out;
}

/** One line per stage: what it does now, for the stepper. */
export function summary(d, t) {
  const out = {};
  const u = d.understand;
  out.understanding = u.values && u.fields.length ? `${u.mode === "filter" ? "filters" : "boosts"} by values named: ${u.fields.join(", ")}` : "off: the words are searched as typed";
  const w = d.words;
  const wl = w.on ? [`words in ${w.fields.map((x) => x.field).join(", ") || "no field"}`, w.match === "all" ? "all needed" : w.match === "relax" ? "all, relaxing" : null, w.typos !== "off" ? "typos" : null, w.synonyms ? "synonyms" : null, Number(w.feedback) > 0 ? `feedback from ${w.feedback} rows` : null].filter(Boolean).join(" · ") : null;
  out.retrieval = [wl, d.meaning.on && d.meaning.field ? `meaning of ${d.meaning.field}` : null, d.filters.length ? `${d.filters.length} filter${d.filters.length === 1 ? "" : "s"}` : null, inputs(d, t).length > 1 ? `top ${d.candidates} each` : null].filter(Boolean).join(" · ");
  const hd = d.rank.heads;
  const hn = hd?.on ? hd.list.length : 0;
  out.ranking = [d.rank.boosts.length ? `${d.rank.boosts.length} boost${d.rank.boosts.length === 1 ? "" : "s"}` : null, hn ? `${hn} head${hn === 1 ? "" : "s"} over the top ${headsDepth(d)}${hn > 1 ? `, ${hd.fuse === "rrf" ? "by rank" : "weighted"}` : ""}` : null].filter(Boolean).join(" · ") || "as retrieved";
  const b = d.blend;
  out.blending = b.on ? [b.diversify?.on ? `diverse on ${b.diversify.on}` : null, b.cap?.per ? `at most ${b.cap.at_most} per ${b.cap.per}` : null, b.dedupe?.on ? "no near duplicates" : null, b.quota?.column ? `quota of ${b.quota.column} = ${b.quota.value}` : null, b.sources.length ? `${b.sources.length + 1} queries` : null].filter(Boolean).join(" · ") || "on, no rules yet" : "off: the ranked rows are the page";
  return out;
}

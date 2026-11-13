// A home per table (#/table/<name>): what a person who owns the data
// comes for, in one place. Search, the box that works on the first try
// (some of the words, ranked; by meaning too once the table has vectors,
// which one click adds); Data, its rows; Quality, how filled each field
// is; Used by, the keys, saved searches, statements and apps that read
// it; Storage, its index (the inspector). A search is kept as a saved
// search (a stored function, an endpoint, an agent's tool) from here.
// #/table lists the tables, the user's own first.

import { h, icon, clear, api, card, empty, failure, fmt, toast, copy, seg, gate, can, sqlBlock, store, debounce } from "../core.js";
import { tables, cached, table as tableOf, typeBadge, invalidate } from "../catalog.js";
import { ownerOf, isMine, folded } from "../owners.js";
import { quoteName } from "../sqledit.js";
import { grid, answerStats, exportMenu } from "../results.js";
import { useFromCode, saveSearch, slug } from "./saved.js";
import { bestText } from "./builder.js";
import { NOT_PROSE } from "./models.js";
import inspector from "./inspector.js";

export default { id: "table", title: "Tables", icon: "table", group: "Build", range: false, keywords: "tables table home search find rows data quality filled empty used by keys storage index meaning vectors semantic", render, onParams };

const TABS = [["search", "Search"], ["data", "Data"], ["quality", "Quality"], ["used", "Used by"], ["storage", "Storage"]];
let again = null;
function onParams(r) { again?.(r); }

const searchable = (f) => f && f.type === "text" && f.indexed !== false;
/** The text fields a search looks in: prose, not addresses, the best first, at most four. */
export function proseFields(t) {
  const fs = (t?.short?.fields || []).filter(searchable);
  const prose = fs.filter((f) => !NOT_PROSE.test(f.name));
  const best = bestText(prose.length ? prose : fs);
  const title = prose.find((f) => /^(title|name|subject|headline)$/i.test(f.name));
  return [...new Set([best, title, ...prose].filter(Boolean))].slice(0, 4);
}
/** The vector fields a table embeds from its text (searchable by words). */
const meaningFields = (t) => (t?.short?.fields || []).filter((f) => f.type === "vector" && f.embed && !f.multi && !/_(next|prev)$/.test(f.name));
/** The fields an answer shows: the key, a title, the text searched, a few more; never vectors. */
function shownFields(t, text) {
  const fs = (t?.short?.fields || []).filter((f) => !["vector", "sparse", "edges"].includes(f.type));
  const key = t.short?.key;
  const want = [key, ...text.map((f) => f.name), ...fs.filter((f) => f.type !== "text" && f.type !== "json").map((f) => f.name)];
  return [...new Set(want.filter((n) => n && fs.some((f) => f.name === n)))].slice(0, 7);
}

/**
 * The search a box runs: {sql, params, body, fnParams}. `mode`: words,
 * meaning or both. Words alone over one field rank by bm25 with a guard
 * (all the words while 20 rows have them, else fewer), so a sentence
 * finds rows; over several, a row needs a word of one of them and the
 * scores add up. Meaning ranks every row by similarity; both fuse the
 * rankings (rrf). `body` and `fnParams` are the same search as a stored
 * function's (the words :q, the vector :v, the count :k).
 */
export function searchSql(t, { q, fields, vec, mode, limit = 20 }) {
  const cols = shownFields(t, fields).map(quoteName);
  const from = quoteName(t.table);
  const W = fields.map((f) => quoteName(f.name));
  let where = "", order;
  const words = mode !== "meaning" && W.length;
  const meaning = mode !== "words" && vec;
  if (words && meaning) order = `rrf(${W.map((f) => `bm25(${f}, :q)`).join(", ")}, similarity(${quoteName(vec.name)}, :v))`;
  else if (meaning) order = `similarity(${quoteName(vec.name)}, :v)`;
  else if (W.length === 1) order = `bm25(${W[0]}, :q, min => 1, min_rows => 20)`;
  else if (W.length) { where = `\nWHERE ${W.map((f) => `MATCH(${f}, :q, min => 1)`).join("\n   OR ")}`; order = W.map((f) => `bm25(${f}, :q)`).join(" + "); }
  else return null;
  const head = `SELECT ${[...cols, "score()"].join(", ")}\nFROM ${from}${where}\nORDER BY ${order} DESC`;
  const params = {};
  if (words) params.q = q;
  if (meaning) params.v = { embed: q, field: vec.name };
  const fnParams = [];
  if (words) fnParams.push({ name: "q", ty: "TEXT" });
  if (meaning) fnParams.push({ name: "v", ty: `VECTOR(${vec.dim || ""})`.replace("()", "") });
  fnParams.push({ name: "k", ty: "INT", default: 10 });
  return { sql: `${head}\nLIMIT ${limit}`, params, body: `${head}\nLIMIT :k`, fnParams };
}

// ------------------------------------------------------------ the page
function render(page, r) {
  const body = h("div", { class: "stack" });
  page.append(body);
  let stop = null, alive = true;

  function show(route) {
    r = route;
    stop?.(); stop = null;
    const name = r.rest?.[0];
    if (!name) return index();
    tables().then(() => {
      const t = tableOf(name);
      if (!alive) return;
      if (!t) { clear(body, card({ body: empty("table", `No table ${name}`, "It may have been dropped, or your key does not read it.", h("a", { class: "btn", href: "#/table" }, "All tables")) })); return; }
      tablePage(t, r.params.get("tab") || store.get("table.tab", "search"));
    }).catch((e) => clear(body, failure(e, "The tables")));
  }
  again = show;

  // ---------------------------------------------------------- every table
  async function index() {
    clear(body, h("div", { class: "page-head" }, h("div", {}, h("h1", {}, "Tables"), h("p", {}, "Your tables first: search one, look at its rows, see who uses it.")),
      h("div", { class: "tools" }, gate(h("a", { class: "btn", href: "#/import" }, icon("import"), "Import a file"), "write"), gate(h("a", { class: "btn", href: "#/schema?new=1" }, icon("plus"), "Design a table"), "admin"))),
    h("div", { class: "small muted", style: { padding: "12px" } }, "Reading the tables…"));
    let ts;
    try { ts = await tables(true); } catch (e) { clear(body.lastChild, failure(e, "The tables")); return; }
    const sizes = {};
    try { const j = await api("/v1/index/tables", { timeout: 4000 }); for (const x of Array.isArray(j) ? j : j.tables || []) sizes[x.table] = x.docs; } catch { /* a server without the listing */ }
    const mine = ts.filter((t) => isMine(t.table));
    await Promise.all(mine.filter((t) => sizes[t.table] == null).slice(0, 30).map((t) => api("/v1/query", { body: { sql: `SELECT COUNT(*) FROM ${quoteName(t.table)}` }, timeout: 4000 }).then((j) => { sizes[t.table] = j.rows?.[0]?.[0]; }).catch(() => {})));
    if (!alive || r.rest?.[0]) return;
    const tile = (t) => {
      const fs = t.short?.fields || [];
      const text = proseFields(t), vecs = meaningFields(t);
      const o = ownerOf(t.table);
      return h("a", { class: "tbl-tile", href: `#/table/${encodeURIComponent(t.table)}` },
        h("div", { class: "row nw" }, icon("table"), h("b", { class: "grow ellipsis" }, t.table), sizes[t.table] != null ? h("span", { class: "small muted" }, fmt.si(sizes[t.table]) + " rows") : null),
        h("div", { class: "small dim ellipsis" }, `${fmt.plural(fs.length, "field")}${text.length ? " · search " + text.map((f) => f.name).join(", ") : " · no text to search"}`),
        h("div", { class: "row nw small", style: { gap: "6px", marginTop: "4px" } },
          text.length ? h("span", { class: "badge" }, "words") : null, vecs.length ? h("span", { class: "badge accent" }, "meaning") : text.length ? h("span", { class: "badge muted", title: "Add vectors to search by meaning" }, "no meaning yet") : null,
          o ? h("span", { class: "badge", title: o.about }, o.owner) : null));
    };
    const list = folded(ts, tile, { key: "tablehome", nameOf: (t) => t.table, label: "Tables of apps" });
    clear(body.lastChild, ts.length ? h("div", { class: "tbl-tiles" }, list) : card({ body: empty("table", "No tables yet", "Import a file to make one, or design one.", h("div", { class: "row" }, h("a", { class: "btn primary", href: "#/import" }, icon("import"), "Import a file"), h("a", { class: "btn", href: "#/start" }, icon("rocket"), "Start here"))) }));
  }

  // ---------------------------------------------------------- one table
  function tablePage(t, tab) {
    if (!TABS.some((x) => x[0] === tab)) tab = "search";
    store.set("table.tab", tab);
    const o = ownerOf(t.table);
    const tabsEl = h("div", { class: "tabs", role: "tablist" }, TABS.map(([k, label]) => h("button", { role: "tab", "aria-selected": String(k === tab), onclick: () => { location.hash = `#/table/${encodeURIComponent(t.table)}?tab=${k}`; } }, label)));
    const pane = h("div", { class: "stack" });
    const rows = h("span", { class: "small muted" });
    clear(body,
      h("div", { class: "row" }, h("a", { class: "btn ghost sm", href: "#/table" }, icon("left"), "All tables")),
      h("div", { class: "page-head" }, h("div", {}, h("h1", { class: "mono" }, t.table), h("p", {}, o ? `Kept by ${o.owner}: ${o.about}. ` : "", rows)),
        h("div", { class: "tools" }, h("a", { class: "btn sm", href: `#/sql?q=${encodeURIComponent(`SELECT * FROM ${quoteName(t.table)} LIMIT 20;`)}` }, icon("sql"), "SQL"), h("a", { class: "btn sm", href: `#/builder?table=${encodeURIComponent(t.table)}` }, icon("builder"), "Builder"), h("a", { class: "btn sm", href: `#/schema?table=${encodeURIComponent(t.table)}` }, icon("schema"), "Schema"))),
      tabsEl, pane);
    api("/v1/query", { body: { sql: `SELECT COUNT(*) FROM ${quoteName(t.table)}` }, timeout: 5000 }).then((j) => { rows.textContent = `${fmt.n(j.rows?.[0]?.[0] ?? 0)} rows · ${fmt.plural((t.short?.fields || []).length, "field")}`; }).catch(() => {});
    stop = { search: searchTab, data: dataTab, quality: qualityTab, used: usedTab, storage: storageTab }[tab](pane, t) || null;
  }

  // ---------------------------------------------------------- Search
  function searchTab(pane, t) {
    const text = proseFields(t);
    let vecs = meaningFields(t);
    const key = `table.search.${t.table}`;
    const saved = store.get(key, {});
    let picked = new Set((saved.fields || text.map((f) => f.name)).filter((n) => text.some((f) => f.name === n)));
    if (!picked.size && text[0]) picked.add(text[0].name);
    let mode = saved.mode || (vecs.length && text.length ? "both" : vecs.length ? "meaning" : "words");
    const inp = h("input", { class: "input search-big", placeholder: text.length || vecs.length ? `Search ${t.table}: a question or some words` : "This table has no text to search", value: saved.q || "", "aria-label": `Search ${t.table}` });
    const modeBox = h("div");
    const fieldsBox = h("div", { class: "row nw small", style: { gap: "6px", flexWrap: "wrap" } });
    const meaningBox = h("div");
    const out = h("div");
    const tail = h("div", { class: "stack" });
    let running = null, last = null;
    function drawControls() {
      vecs = meaningFields(tableOf(t.table) || t);
      if (!vecs.length && mode !== "words") mode = "words";
      if (!text.length && mode !== "meaning") mode = vecs.length ? "meaning" : "words";
      clear(modeBox, seg([["words", "Words", "Rows with your words, the more and rarer the higher"], ["meaning", "Meaning", vecs.length ? `Rows close in meaning (${vecs[0].embed.model})` : "Add meaning first"], ["both", "Both", "Words and meaning fused: the best of the two rankings"]].filter(([k]) => (k === "words" ? text.length : vecs.length) && (k !== "both" || text.length)), mode, (v) => { mode = v; keep(); go(); }, { "aria-label": "Search by" }));
      clear(fieldsBox, mode === "meaning" ? h("span", { class: "muted" }, `by meaning, with ${vecs[0]?.name} (${vecs[0]?.embed?.model})`) : [h("span", { class: "muted" }, "in"), text.map((f) => h("button", { class: "chip", "aria-pressed": String(picked.has(f.name)), title: picked.has(f.name) ? `Stop searching ${f.name}` : `Search ${f.name} too`, onclick: () => { if (picked.has(f.name) && picked.size > 1) picked.delete(f.name); else picked.add(f.name); keep(); drawControls(); go(); } }, typeBadge(f), f.name))]);
      clear(meaningBox, vecs.length || !text.length ? null : meaningOffer(t, () => { invalidate(); tables(true).then(() => { mode = "both"; drawControls(); go(); }); }));
    }
    const keep = () => store.set(key, { q: inp.value, mode, fields: [...picked] });
    async function go() {
      const q = inp.value.trim();
      keep();
      if (!q) { clear(out, empty("search", "Type what you are looking for", "A question works: rows with some of its words come back, those with all of them first.")); clear(tail); return; }
      const s = searchSql(t, { q, fields: text.filter((f) => picked.has(f.name)), vec: vecs[0], mode });
      if (!s) return;
      running?.abort();
      const mine = running = new AbortController();
      out.classList.add("loading");
      try {
        const j = await api("/v1/query", { body: { sql: s.sql, params: s.params }, signal: mine.signal });
        if (mine !== running) return;
        last = { s, j, q };
        clear(out, results(t, j, q, text));
        drawTail();
      } catch (e) { if (e.name !== "AbortError") clear(out, h("div", { class: "error" }, e.message)); }
      finally { if (mine === running) { out.classList.remove("loading"); running = null; } }
    }
    function drawTail() {
      if (!last) return;
      const { s, q } = last;
      const callParams = { ...(s.params.q != null ? { q } : {}), ...(s.params.v ? { v: { embed: q } } : {}), k: 10 };
      clear(tail,
        h("div", { class: "row", style: { gap: "8px", flexWrap: "wrap" } },
          gate(h("button", { class: "btn primary", title: "Keep it as a stored function: an endpoint, a line of code, an agent's tool", onclick: async () => { const n = await saveSearch({ suggested: `search_${t.table}`, description: `Finds rows of ${t.table} about a question${s.params.v ? ", by words and meaning" : ""}.`, params: s.fnParams, body: s.body, table: t.table }); if (n) location.hash = `#/saved/${n}`; } }, icon("bookmark"), "Save as a saved search"), "admin"),
          h("a", { class: "btn", href: `#/sql?q=${encodeURIComponent(s.sql.replace(/:q\b/g, `'${q.replace(/'/g, "''")}'`))}${s.params.v ? `&p=${encodeURIComponent(JSON.stringify({ v: { kind: "embed", value: q, field: s.params.v.field } }))}` : ""}` }, icon("sql"), "Open in the workbench"),
          h("button", { class: "btn", onclick: () => copy(s.sql, "SQL copied") }, icon("copy"), "Copy the SQL")),
        h("details", { class: "card-ish" }, h("summary", {}, "The query"), sqlBlock(s.sql), h("div", { class: "small muted", style: { marginTop: "6px" } }, Object.keys(s.params).length ? `Parameters: ${JSON.stringify(s.params)}` : "")),
        h("details", { class: "card-ish" }, h("summary", {}, "Use this search from code"), useFromCode({ sql: s.sql, params: s.params })));
    }
    inp.oninput = debounce(go, 250);
    inp.onkeydown = (e) => { if (e.key === "Enter") go(); };
    pane.append(h("section", { class: "card" }, h("div", { class: "bd stack", style: { gap: "10px" } }, h("div", { class: "row nw search-row", style: { gap: "8px" } }, icon("search"), inp, modeBox), fieldsBox)), meaningBox, out, tail);
    drawControls();
    go();
    setTimeout(() => inp.focus(), 30);
    return () => running?.abort();
  }

  /** The answer of a search as a list: a title, the text with the words marked, the other fields. */
  function results(t, j, q, text) {
    const rows = j.rows || [], cols = j.columns || [];
    if (!rows.length) return empty("search", "No rows", "Other words, fewer of them, or search by meaning.");
    const words = q.toLowerCase().split(/\W+/).filter((w) => w.length > 2);
    const ti = cols.findIndex((c) => /^(title|name|subject|headline)$/i.test(c));
    const bi = cols.findIndex((c) => text.some((f) => f.name === c) && c !== cols[ti]);
    const si = cols.indexOf("score()");
    const ki = cols.indexOf(t.short?.key);
    const marked = (s) => {
      s = String(s ?? "");
      if (!words.length) return s;
      const re = new RegExp(`\\b(${words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "gi");
      const out = []; let i = 0;
      for (const m of s.matchAll(re)) { out.push(s.slice(i, m.index), h("mark", {}, m[0])); i = m.index + m[0].length; }
      out.push(s.slice(i));
      return out;
    };
    const snip = (s) => {
      s = String(s ?? "").replace(/\s+/g, " ");
      const at = words.length ? s.toLowerCase().search(new RegExp(`\\b(${words.join("|")})`)) : -1;
      const from = at > 80 ? at - 60 : 0;
      return (from ? "…" : "") + s.slice(from, from + 260) + (s.length > from + 260 ? "…" : "");
    };
    return h("section", { class: "card hits-card" }, h("div", { class: "hd" }, h("h3", {}, fmt.plural(rows.length, "row")), h("span", { class: "sub" }, answerStats(j)), h("div", { class: "tools" }, exportMenu(j, `chloedb-${t.table}`))),
      h("div", { class: "bd flush" }, h("ol", { class: "hits" }, rows.map((r) => h("li", {},
        h("div", { class: "row nw" }, h("b", { class: "grow" }, ti >= 0 ? marked(r[ti]) : ki >= 0 ? String(r[ki]) : "row"), si >= 0 ? h("span", { class: "small muted mono", title: "score" }, Number(r[si]).toFixed(3)) : null),
        bi >= 0 ? h("div", { class: "small dim" }, marked(snip(r[bi]))) : null,
        h("div", { class: "small muted hit-f" }, cols.map((c, i) => i === ti || i === bi || i === si || r[i] == null || String(r[i]).length > 80 ? null : h("span", {}, h("span", { class: "k" }, c), " ", String(r[i])))))))));
  }

  /** The offer of meaning for a table without vectors: one click, the defaults told. */
  function meaningOffer(t, done) {
    const from = proseFields(t)[0];
    const box = h("section", { class: "card meaning-offer" });
    api("/v1/models").then((j) => {
      const ms = (j.models || []).filter((m) => m.technique === "static");
      const m = ms.find((x) => x.ready && /retrieval/.test(x.name)) || ms.find((x) => x.ready) || ms.find((x) => /retrieval/.test(x.name)) || ms[0];
      if (!m || !from) { box.remove(); return; }
      const taken = new Set((t.short?.fields || []).map((f) => f.name));
      const name = !taken.has("emb") ? "emb" : `${from.name}_emb`;
      const sql = `ALTER TABLE ${quoteName(t.table)} ADD COLUMN ${quoteName(name)} VECTOR${m.dim ? `(${m.dim})` : ""} OPTIONS(embed = '${from.name}', model = '${m.name}')`;
      const btn = gate(h("button", { class: "btn primary" }, icon("sparkle"), "Add meaning"), "admin");
      const status = h("div", { class: "small muted" });
      btn.onclick = async () => {
        btn.disabled = true;
        try {
          await api("/v1/sql", { body: { sql } });
          invalidate();
          for (let i = 0; i < 120 && box.isConnected; i++) {
            const tt = (await tables(true)).find((x) => x.table === t.table);
            const e = (tt?.embeddings || []).find((x) => x.field === name);
            const b = e?.backfill;
            status.textContent = !e ? "Adding the field…" : e.error ? `The model did not open: ${e.error}` : b && b.state !== "done" && b.missing ? `Filling in vectors: ${fmt.n(b.embedded || 0)} done, ${fmt.n(b.missing)} to go…` : e.loaded ? "Ready" : "Opening the model…";
            if (e?.error) break;
            if (e?.loaded && (!b || b.state === "done" || !b.missing)) { toast(`${t.table} searches by meaning now`, "ok"); done(); return; }
            await new Promise((res) => setTimeout(res, 1500));
          }
        } catch (e) { toast(e.message, "bad"); btn.disabled = false; }
      };
      clear(box, h("div", { class: "bd row", style: { gap: "14px", alignItems: "flex-start" } }, icon("sparkle"),
        h("div", { class: "grow stack", style: { gap: "4px" } }, h("b", {}, "Search by meaning too"),
          h("div", { class: "small dim" }, `Finds rows that say the same thing in other words. One click adds a vector field ${name}, embedded from ${from.name} by ${m.name} (${m.dim || "?"} dimensions, a static model: well under a millisecond a text on a CPU${m.ready ? "" : "; the server fetches it first"}). The rows already there get their vectors in the background.`),
          h("details", { class: "small" }, h("summary", { class: "muted" }, "The statement, and other choices"), sqlBlock(sql), h("div", { style: { marginTop: "6px" } }, "Another model, index or coding: ", h("a", { href: "#/models" }, "Models"), ".")), status),
        btn));
    }).catch(() => box.remove());
    return box;
  }

  // ---------------------------------------------------------- Data
  function dataTab(pane, t) {
    const fs = (t.short?.fields || []).filter((f) => !["vector", "sparse"].includes(f.type)).map((f) => quoteName(f.name));
    const key = t.short?.key;
    const sql = `SELECT ${fs.join(", ") || "*"} FROM ${quoteName(t.table)}${key ? ` ORDER BY ${quoteName(key)}` : ""} LIMIT 100`;
    const out = h("div", { class: "small muted", style: { padding: "12px" } }, "Reading rows…");
    pane.append(h("section", { class: "card" }, h("div", { class: "hd" }, h("h3", {}, "The first rows"), h("span", { class: "sub" }, "vectors left out"), h("div", { class: "tools" }, h("a", { class: "btn sm", href: `#/sql?q=${encodeURIComponent(sql)}` }, icon("sql"), "Open in the workbench"), gate(h("a", { class: "btn sm", href: "#/import" }, icon("import"), "Add rows"), "write"))), h("div", { class: "bd flush" }, out)));
    api("/v1/query", { body: { sql } }).then((j) => clear(out, j.rows?.length ? h("div", { class: "tbl-wrap" }, grid(j, { max: 100 })) : empty("table", "No rows yet", "Import a file into it, or write rows from code.", gate(h("a", { class: "btn", href: "#/import" }, icon("import"), "Import into it"), "write"))))
      .catch((e) => clear(out, h("div", { class: "error" }, e.message)));
  }

  // ---------------------------------------------------------- Quality
  function qualityTab(pane, t) {
    const fs = (t.short?.fields || []).filter((f) => !["edges", "vector"].includes(f.type));
    const out = h("div", { class: "small muted", style: { padding: "12px" } }, "Counting…");
    pane.append(h("section", { class: "card" }, h("div", { class: "hd" }, h("h3", {}, "How filled each field is"), h("span", { class: "sub" }, "the rows where it is set")), h("div", { class: "bd" }, out)));
    (async () => {
      const tq = quoteName(t.table);
      const n = (await api("/v1/query", { body: { sql: `SELECT COUNT(*) FROM ${tq}` } })).rows?.[0]?.[0] || 0;
      if (!n) { clear(out, empty("table", "No rows to judge", "Quality shows once rows are in.")); return; }
      const counts = await Promise.all(fs.map((f) => api("/v1/query", { body: { sql: `SELECT COUNT(*) FROM ${tq} WHERE ${quoteName(f.name)} IS NOT NULL` }, timeout: 8000 }).then((j) => j.rows?.[0]?.[0] ?? null).catch(() => null)));
      const distinct = await Promise.all(fs.map((f) => f.type === "keyword" && !f.multi ? api("/v1/query", { body: { sql: `SELECT ${quoteName(f.name)}, COUNT(*) AS n FROM ${tq} GROUP BY ${quoteName(f.name)} ORDER BY n DESC LIMIT 3` }, timeout: 8000 }).then((j) => j.rows || []).catch(() => null) : null));
      const notes = [];
      const rowsEl = fs.map((f, i) => {
        const c = counts[i];
        const share = c == null ? null : c / n;
        if (share != null && share < 0.5 && f.name !== t.short?.key) notes.push(`${f.name} is empty in ${fmt.pct(1 - share, 0)} of the rows`);
        const top = distinct[i];
        if (top?.length && top[0][1] / n > 0.9 && n > 20) notes.push(`${f.name} holds one value (${top[0][0]}) in ${fmt.pct(top[0][1] / n, 0)} of the rows`);
        return h("div", { class: "q-row" }, h("span", { class: "row nw" }, typeBadge(f), h("b", { class: "mono" }, f.name)),
          h("span", { class: "q-bar", title: share == null ? "could not count" : `${fmt.n(c)} of ${fmt.n(n)}` }, h("i", { style: { width: share == null ? "0" : Math.max(1, share * 100) + "%", background: share != null && share < 0.5 ? "var(--serious)" : "var(--accent)" } })),
          h("span", { class: "small mono" }, share == null ? "–" : fmt.pct(share, share < 1 && share > 0.99 ? 1 : 0)),
          h("span", { class: "small muted ellipsis" }, top?.length ? "most: " + top.map((x) => `${x[0] ?? "null"} (${fmt.n(x[1])})`).join(", ") : ""));
      });
      for (const e of t.embeddings || []) if (e.backfill?.missing) notes.push(`${e.field} has no vector yet for ${fmt.n(e.backfill.missing)} rows (being filled in)`);
      if (!proseFields(t).length) notes.push("No text field to search: words search needs a TEXT field");
      clear(out, notes.length ? h("ul", { class: "q-notes" }, notes.map((x) => h("li", {}, icon("warn"), " ", x))) : h("p", { class: "small", style: { marginTop: 0 } }, icon("okc"), " Every field is set in most rows."), h("div", { class: "q-rows" }, rowsEl));
    })().catch((e) => clear(out, h("div", { class: "error" }, e.message)));
  }

  // ---------------------------------------------------------- Used by
  function usedTab(pane, t) {
    const out = h("div", { class: "stack" }, h("div", { class: "small muted", style: { padding: "12px" } }, "Looking…"));
    pane.append(out);
    const re = new RegExp(`(^|[^a-z0-9_])"?${t.table.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"?([^a-z0-9_]|$)`, "i");
    Promise.allSettled([api("/v1/functions"), can("admin") ? api("/v1/admin/keys") : Promise.reject(new Error("needs an admin key")), api("/v1/queries/stats?limit=500")]).then(([fn, ks, st]) => {
      const fns = fn.status === "fulfilled" ? (fn.value.functions || []).filter((f) => f.current != null && f.versions.some((v) => v.version === f.current && re.test(v.body || ""))) : [];
      const keys = ks.status === "fulfilled" ? (ks.value.keys || []).filter((k) => !k.revoked_ms && k.tables && Object.keys(k.tables).some((p) => p === "*" || (p.endsWith("*") ? t.table.startsWith(p.slice(0, -1)) : p === t.table))) : null;
      const sts = st.status === "fulfilled" ? (Array.isArray(st.value) ? st.value : st.value.statements || []).filter((s) => re.test(s.sql || s.example || s.fingerprint || "")) : [];
      const byKey = new Map();
      for (const s of sts) for (const k of s.keys || []) if (k.key) byKey.set(k.key, (byKey.get(k.key) || 0) + (k.calls || 0));
      const o = ownerOf(t.table);
      clear(out,
        o ? card({ title: "App", body: h("div", {}, h("b", {}, o.owner), h("span", { class: "dim" }, `: ${o.about}. Its writes come from the app, not from people.`)) }) : null,
        card({ title: "Saved searches", sub: `${fns.length} call this table`, body: fns.length ? h("div", { class: "row", style: { gap: "6px", flexWrap: "wrap" } }, fns.map((f) => h("a", { class: "chip", href: `#/saved/${encodeURIComponent(f.name)}` }, icon("bookmark"), f.name))) : h("div", { class: "small muted" }, "None yet. ", h("a", { href: `#/table/${encodeURIComponent(t.table)}?tab=search` }, "Search it"), ", then save the search.") }),
        card({ title: "Keys limited to it", sub: keys == null ? "needs an admin key" : `${keys.length} keys`, body: keys == null ? h("div", { class: "small muted" }, "The key list needs an admin key; below are the keys seen calling it.") : keys.length ? h("div", { class: "stack", style: { gap: "4px" } }, keys.map((k) => h("div", { class: "row nw small" }, icon("key"), h("b", {}, k.name), h("span", { class: "muted" }, ` ${k.scope}${k.owner ? " · owner " + k.owner : ""} · ${k.last_used_ms ? "used " + fmt.ago(k.last_used_ms) : "never used"}`)))) : h("div", { class: "small muted" }, "No key is limited to this table; keys without limits read every table.") }),
        card({ title: "Who queried it lately", sub: "the statements of the last 15 minutes that name it", flush: !!sts.length, body: sts.length ? h("div", { class: "stack", style: { gap: "0" } },
          byKey.size ? h("div", { class: "row small", style: { gap: "6px", padding: "10px 14px", flexWrap: "wrap" } }, "Keys: ", [...byKey].sort((a, b) => b[1] - a[1]).map(([k, n]) => h("a", { class: "chip", href: `#/queries?key=${encodeURIComponent(k)}` }, icon("key"), `${k} · ${fmt.plural(n, "call")}`))) : null,
          sts.slice(0, 12).map((s) => h("a", { class: "used-st", href: `#/statement/${encodeURIComponent(s.fingerprint || s.sql)}` }, h("span", { class: "mono small ellipsis grow" }, s.sql || s.example || s.fingerprint), h("span", { class: "small muted" }, `${fmt.plural(s.calls || 0, "call")}${s.p99_ms != null ? " · p99 " + fmt.ms(s.p99_ms) : ""}`)))) : h("div", { class: "small muted" }, "No statement named it in the last 15 minutes.") }));
    });
  }

  // ---------------------------------------------------------- Storage
  function storageTab(pane, t) {
    const host = h("div");
    pane.append(host);
    try {
      const c = inspector.render(host, { id: "inspector", rest: [], params: new URLSearchParams({ table: t.table }) });
      return typeof c === "function" ? c : null;
    } catch (e) { clear(host, failure(e, "The storage")); return null; }
  }

  show(r);
  return () => { alive = false; again = null; stop?.(); };
}

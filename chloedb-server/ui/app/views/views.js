// Team views (#/views, #/views/<id>): a query built in the builder or
// written in the workbench, kept on the server for everyone who reads it
// (GRACE-407). A view is a row of table cc_views: its name, what it shows,
// its SQL and parameters, how its chart is drawn, and for a builder's view
// the builder's state, so that it opens there again. A read key lists and
// opens the views, a write key saves them, and the first save makes the
// table, which takes an admin's key. Its link opens it, on a phone too:
// the answer as a chart and a table, with the counts the SQL gives.

import { h, icon, clear, api, card, empty, failure, fmt, toast, copy, modal, confirm, gate, can, sqlBlock, whoName, ApiError } from "../core.js";
import { isProduction } from "../data.js";
import { grid, answerStats, exportMenu, resultChart } from "../results.js";
import { slug } from "./saved.js";

export default { id: "views", title: "Team views", icon: "eye", group: "Build", range: false, keywords: "team views saved view dashboard report monday numbers chart share link phone builder workbench kept on the server everyone", render, onParams };

/** The table the views are rows of. */
export const TABLE = "cc_views";
/** The table, made by the first save. */
export const TABLE_SQL = `CREATE TABLE ${TABLE} (id KEYWORD PRIMARY KEY, name TEXT, about TEXT, kind KEYWORD, sql TEXT, spec JSON, by KEYWORD, at TIMESTAMP)`;

let reshow = null;
function onParams(r) { reshow?.(r); }

/** A view's link, the one Share copies. */
export const viewLink = (id) => `${location.origin}${location.pathname}#/views/${encodeURIComponent(id)}`;

/**
 * A view as the row it is kept as: `kind` builder or sql, `sql` and
 * `params` what it runs, `chart` how its chart is drawn ({x, ys, split,
 * kind}, or null for a table), `state` the builder's. Its id is its name's
 * slug, so saving a view of the same name again replaces it.
 */
export function viewDoc({ name, about = "", kind, sql, params = {}, chart = null, state = null, by = null, at = Date.now() }) {
  const n = String(name || "").trim();
  if (!n) throw new Error("A view needs a name");
  if (!String(sql || "").trim()) throw new Error("A view needs a query");
  return { id: slug(n), name: n, about: String(about || "").trim(), kind, sql: String(sql).trim(), spec: { params: params || {}, chart: chart || null, state: state || null }, by, at: new Date(at).toISOString() };
}

/** Whether an error says the views' table is not there yet. */
export const noTable = (e) => e instanceof ApiError && (e.body?.error_code === "table_not_found" || /no such table/i.test(e.message || "")) && String(e.message || "").includes(TABLE);

/** Every view, the newest first: [{id, name, about, kind, by, at}], none before the first save. */
export async function listViews() {
  try {
    const j = await api("/v1/query", { body: { sql: `SELECT id, name, about, kind, by, at FROM ${TABLE} ORDER BY at DESC LIMIT 1000` } });
    return (j.rows || []).map(([id, name, about, kind, by, at]) => ({ id, name, about, kind, by, at }));
  } catch (e) { if (noTable(e)) return []; throw e; }
}

/** One view, every field, or null. */
export async function getView(id) {
  try {
    const j = await api("/v1/query", { body: { sql: `SELECT id, name, about, kind, sql, spec, by, at FROM ${TABLE} WHERE id = :id LIMIT 1`, params: { id } } });
    const r = j.rows?.[0];
    if (!r) return null;
    const [, name, about, kind, sql, spec, by, at] = r;
    const sp = typeof spec === "string" ? JSON.parse(spec) : spec || {};
    return { id, name, about, kind, sql, params: sp.params || {}, chart: sp.chart || null, state: sp.state || null, by, at };
  } catch (e) { if (noTable(e)) return null; throw e; }
}

/** Keeps a view (its doc, as viewDoc makes it): the table made first when there is none. */
export async function putView(doc, reason) {
  const write = () => api("/v1/write", { body: { table: TABLE, ops: [{ op: "upsert", doc }] }, reason });
  try { await write(); }
  catch (e) {
    if (!noTable(e)) throw e;
    if (!can("admin")) throw new Error(`There is no ${TABLE} table yet: the first view is saved with an admin's key, which makes it`);
    await api("/v1/sql", { body: { sql: TABLE_SQL }, reason: reason || "the first team view: the table that keeps them" });
    await write();
  }
  return doc.id;
}

/**
 * Asks a view's name and what it shows, then keeps it: `what` is what
 * viewDoc takes but its name. Resolves the view's id, or null.
 */
export function saveView(what, { suggested = "" } = {}) {
  return new Promise((resolve) => {
    const nameIn = h("input", { class: "input", value: suggested, placeholder: "Monday numbers", style: { width: "100%" } });
    const aboutIn = h("input", { class: "input", value: what.about || "", placeholder: "What it shows, for the team", style: { width: "100%" } });
    const note = h("div", { class: "small muted" });
    let names = null;
    listViews().then((vs) => { names = new Map(vs.map((v) => [v.id, v])); sync(); }).catch(() => {});
    const ok = h("button", { class: "btn primary" }, icon("eye"), "Save the view");
    function sync() {
      const id = slug(nameIn.value), was = names?.get(id);
      clear(note, was ? h("span", { style: { color: "var(--serious)" } }, icon("warn"), ` ${was.name} exists${was.by ? ` (saved by ${was.by})` : ""}: saving replaces it for everyone.`) : `It is kept on this server as a row of ${TABLE}: everyone with a key that reads it lists and opens it, by its link too, on a phone as well.`);
      ok.lastChild.textContent = was ? "Replace the view" : "Save the view";
    }
    nameIn.oninput = sync;
    const m = modal({
      title: "Save as a team view",
      body: [h("label", { class: "f" }, "Name", nameIn), h("label", { class: "f" }, "What it shows", aboutIn), note,
        h("details", {}, h("summary", { class: "small muted" }, "The query it runs"), sqlBlock(what.sql))],
      foot: [h("button", { class: "btn", onclick: () => { m.close(); resolve(null); } }, "Cancel"), gate(ok, "write")],
    });
    ok.onclick = async () => {
      let doc;
      try { doc = viewDoc({ ...what, name: nameIn.value, about: aboutIn.value, by: whoName() }); } catch (e) { toast(e.message, "bad"); nameIn.focus(); return; }
      ok.disabled = true;
      try {
        await putView(doc, isProduction() ? `team view ${doc.name}` : undefined);
        m.close();
        copy(viewLink(doc.id), `Saved ${doc.name}: its link is copied`);
        resolve(doc.id);
      } catch (e) { ok.disabled = false; toast(e.message, "bad", 8000); }
    };
    sync();
  });
}

// ------------------------------------------------------------ the page
function render(page, r) {
  const head = h("div", { class: "page-head" },
    h("div", {}, h("h1", {}, "Team views"), h("p", {}, "Queries kept on this server for the team: built in the Query builder or written in the SQL workbench, opened by anyone whose key reads them, by a link, on a phone too.")),
    h("div", { class: "tools" }, h("a", { class: "btn", href: "#/builder" }, icon("builder"), "Build one"), h("a", { class: "btn", href: "#/sql" }, icon("sql"), "Write one")));
  const body = h("div", { class: "stack" });
  page.append(head, body);
  let alive = true;

  function show(route) {
    r = route;
    const id = r.rest?.[0];
    if (id) return detail(id);
    clear(body, h("div", { class: "small muted", style: { padding: "12px" } }, "Reading the views…"));
    listViews().then((vs) => {
      if (!alive) return;
      if (!vs.length) { clear(body, card({ body: empty("eye", "No team views yet", "Build a query in the Query builder (a count by week, a median, a share), or write one in the SQL workbench, then Save as a view: it is kept here for the team.", h("a", { class: "btn primary", href: "#/builder" }, icon("builder"), "Open the Query builder")) })); return; }
      clear(body, card({ title: "Views", sub: `${fmt.plural(vs.length, "view")} · kept in ${TABLE} on this server`, flush: true,
        body: h("div", { class: "saved-list" }, vs.map((v) => h("a", { class: "saved-row", href: `#/views/${encodeURIComponent(v.id)}` },
          h("div", { class: "grow", style: { minWidth: 0 } }, h("div", {}, h("b", {}, v.name), h("span", { class: "small muted" }, ` · ${v.kind === "builder" ? "from the builder" : "SQL"}`)),
            v.about ? h("div", { class: "small dim" }, v.about) : null),
          h("div", { class: "small muted", style: { textAlign: "right", minWidth: "90px" } }, v.at ? fmt.ago(Date.parse(v.at)) : "", v.by ? h("div", {}, "by " + v.by) : null),
          icon("right")))) }));
    }).catch((e) => clear(body, failure(e, "The team views")));
  }
  reshow = (route) => { if (alive) show(route); };

  async function detail(id) {
    clear(body, h("div", { class: "small muted", style: { padding: "12px" } }, "Opening the view…"));
    let v;
    try { v = await getView(id); } catch (e) { clear(body, failure(e, "The view")); return; }
    if (!alive) return;
    if (!v) { clear(body, card({ body: empty("eye", `No view ${id}`, "It may have been removed, or your key does not read it.", h("a", { class: "btn", href: "#/views" }, "All views")) })); return; }
    const out = h("div", {}, h("div", { class: "small muted", style: { padding: "12px" } }, "Running it…"));
    const openIt = v.kind === "builder" && v.state
      ? h("a", { class: "btn sm", href: `#/builder?view=${encodeURIComponent(v.id)}`, title: "Open it in the Query builder to change it" }, icon("builder"), "Open in the builder")
      : h("a", { class: "btn sm", href: `#/sql?q=${encodeURIComponent(v.sql)}`, title: "Open its SQL in the workbench" }, icon("sql"), "Open in the workbench");
    clear(body,
      h("div", { class: "row" }, h("a", { class: "btn ghost sm", href: "#/views" }, icon("left"), "All views")),
      card({ title: v.name, sub: [v.about, v.by ? `saved by ${v.by}` : null, v.at ? fmt.ago(Date.parse(v.at)) : null].filter(Boolean).join(" · "),
        tools: [h("button", { class: "btn sm", title: "Copy this view's link", onclick: () => copy(viewLink(v.id), `Link copied: ${v.name}. It holds no key.`) }, icon("link"), "Copy link"), openIt, gate(h("button", { class: "btn sm danger", onclick: () => drop(v) }, icon("trash"), "Remove"), "write")],
        body: out }),
      h("details", { class: "card-ish" }, h("summary", {}, "The query"), sqlBlock(v.sql)));
    try {
      const j = await api("/v1/query", { body: { sql: v.sql, params: v.params } });
      if (!alive) return;
      const n = j.rows?.length || 0;
      clear(out, h("div", { class: "stack", style: { gap: "10px" } },
        n && v.chart ? resultChart(j, { init: v.chart, compact: true }) : null,
        h("div", { class: "row", style: { gap: "10px", flexWrap: "wrap" } }, answerStats(j), h("span", { class: "sep grow" }), n ? exportMenu(j, slug(v.name)) : null),
        n ? h("div", { class: "tbl-wrap" }, grid(j, { max: 1000 })) : empty("search", "No rows", "The view's query matches nothing now.")));
    } catch (e) { clear(out, h("div", { class: "error" }, e.message)); }
  }

  async function drop(v) {
    const ok = await confirm({ title: `Remove ${v.name}?`, body: "It goes for everyone: its link opens nothing from then on. The tables it reads are not touched.", ok: "Remove", danger: true, reason: isProduction() ? "Why: kept in the audit log" : false });
    if (!ok) return;
    try { await api("/v1/write", { body: { table: TABLE, ops: [{ op: "delete", key: v.id }] }, reason: ok.reason || undefined }); toast(`${v.name} removed`, "ok"); location.hash = "#/views"; } catch (e) { toast(e.message, "bad"); }
  }

  show(r);
  return () => { alive = false; reshow = null; };
}

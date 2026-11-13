// An answer bigger than a query returns (--max-result-window, 10,000 rows
// by default): the workbench shows its first page, read as a scroll, says
// how many rows the query matches, and writes every row to a file on
// "Export all", a page at a time (a CSV), or the whole table at once from
// GET /v1/export (Parquet) when the statement reads a table whole and the
// key may export.

import { h, icon, api, fmt, toast, ApiError } from "./core.js";
import { csvLines } from "./results.js";

/** The rows a page holds: the most a query returns by default. */
export const SHOWN = 10000;

/** Whether an error is the server refusing an answer for its size. */
export const tooBig = (e) => e instanceof ApiError && e.status === 400 && /the most a query returns|a query returns at most/.test(e.message || "");

const closeCursor = (c) => c && api(`/v1/query/cursors/${encodeURIComponent(c)}`, { method: "DELETE" }).catch(() => {});

/**
 * The first SHOWN rows of a query the server refused whole, as a scroll's
 * first page (whose cursor is closed), with `capped`: {shown, total?}.
 * `total` is COUNT(*) of the statement, left out when that fails.
 */
export async function firstPage(sql, params, signal) {
  const j = await api("/v1/query", { body: { sql, params, page_size: SHOWN }, signal });
  closeCursor(j.cursor);
  delete j.cursor;
  let total = null;
  if (j.rows && j.rows.length >= SHOWN) {
    try {
      const c = await api("/v1/query", { body: { sql: `SELECT COUNT(*) AS n FROM (${sql}) AS whole`, params }, signal, timeout: 30000 });
      const n = c.rows?.[0]?.[0];
      if (Number.isFinite(n)) total = n;
    } catch { /* the count is a nicety: "more than" instead */ }
  }
  j.capped = { shown: j.rows?.length || 0, total };
  return j;
}

/** The table a statement reads whole (`SELECT * FROM t`), or null. */
export function wholeTable(sql) {
  const m = /^\s*SELECT\s+\*\s+FROM\s+("[^"]+"|[A-Za-z_][\w]*)\s*;?\s*$/i.exec(sql);
  return m ? m[1].replace(/^"|"$/g, "") : null;
}

/** Where a file goes: a stream to disk where the browser has one, else memory, then a download. */
async function sink(name, type) {
  if (window.showSaveFilePicker) {
    try {
      const fh = await window.showSaveFilePicker({ suggestedName: name });
      const w = await fh.createWritable();
      return { write: (t) => w.write(t), close: () => w.close(), abort: () => w.abort?.() };
    } catch (e) { if (e?.name === "AbortError") return null; /* not allowed here: memory */ }
  }
  const parts = [];
  return {
    write: (t) => { parts.push(t); },
    close: () => { const a = h("a", { href: URL.createObjectURL(new Blob(parts, { type })), download: name }); document.body.append(a); a.click(); a.remove(); },
    abort: () => { parts.length = 0; },
  };
}

/** Every row of `sql` as a CSV, read a page at a time; `onPage(rows)` follows it. */
export async function exportCsv(sql, params, name, { onPage, signal } = {}) {
  const out = await sink(name, "text/csv");
  if (!out) return null;
  let cursor = null, rows = 0;
  try {
    let j = await api("/v1/query", { body: { sql, params, page_size: SHOWN }, signal });
    await out.write(csvLines([j.columns || []]));
    for (;;) {
      await out.write(csvLines(j.rows || []));
      rows += j.rows?.length || 0;
      onPage?.(rows);
      cursor = j.cursor;
      if (!cursor) break;
      j = await api("/v1/query", { body: { cursor }, signal });
    }
    cursor = null;
    await out.close();
    return rows;
  } catch (e) { closeCursor(cursor); out.abort(); throw e; }
}

/** A table whole as Parquet from /v1/export, or null when this server or key does not give one. */
export async function exportTable(table, name) {
  let r;
  try { r = await api(`/v1/export?format=parquet&tables=${encodeURIComponent(table)}`, { blob: true }); }
  catch (e) { if (e instanceof ApiError && [400, 403, 404, 405, 501].includes(e.status)) return null; throw e; }
  const b = await r.blob();
  const a = h("a", { href: URL.createObjectURL(b), download: name });
  document.body.append(a); a.click(); a.remove();
  return b.size;
}

/** The banner over a capped answer: how many of how many, and "Export all". */
export function banner(j, sql, params) {
  const { shown, total } = j.capped;
  const stamp = new Date().toISOString().slice(0, 19).replace(/:/g, "");
  const label = h("span", {}, "Export all");
  let ac = null;
  const btn = h("button", { class: "btn sm", title: "Every row the query matches, written to a file a page at a time" }, icon("download"), label);
  btn.onclick = async () => {
    if (ac) { ac.abort(); return; }
    ac = new AbortController();
    label.textContent = "Stop";
    try {
      const table = wholeTable(sql);
      if (table) {
        const size = await exportTable(table, `${table}-${stamp}.parquet`);
        if (size != null) { toast(`${table}: ${fmt.bytes(size)} of Parquet`, "ok"); return; }
      }
      const n = await exportCsv(sql, params, `chloedb-query-${stamp}.csv`, { signal: ac.signal, onPage: (k) => { label.textContent = `Stop (${fmt.n(k)} rows)`; } });
      if (n != null) toast(`${fmt.plural(n, "row")} written`, "ok");
    } catch (e) {
      toast(e.name === "AbortError" ? "Export stopped: the file was not written" : `Export failed: ${e.message}`, e.name === "AbortError" ? "info" : "bad", 6000);
    } finally { ac = null; label.textContent = "Export all"; }
  };
  return h("div", { class: "wb-capped row", role: "status", style: { gap: "10px", padding: "8px 14px", borderBottom: "1px solid var(--line)", background: "var(--surface-2, transparent)" } },
    icon("info"),
    h("span", { class: "small" }, h("b", {}, `Showing ${fmt.n(shown)} of ${total != null ? fmt.n(total) : `more than ${fmt.n(shown)}`} rows`),
      ": a query returns at most ", fmt.n(shown), " at once. Add a LIMIT or a WHERE, or export them all."),
    h("span", { class: "sep" }), btn);
}

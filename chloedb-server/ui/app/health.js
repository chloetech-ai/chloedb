// The health of an answer's columns, beside its rows in the workbench: for
// each column the rows without a value, the distinct values, the least and
// the most and, for numbers, the spread (standard deviation); a column that
// is always NULL is flagged, and so is a sum that a few rows make (the top
// 1% of rows carrying half of it or more). It is computed by SQL over the
// whole answer (the statement as a derived table), so it covers every row
// the query matches, not only those shown; where the server cannot run that
// (a statement a derived table cannot hold), it is computed from the rows
// shown and says so.

import { h, api, fmt, empty } from "./core.js";

const isNum = (v) => typeof v === "number" && Number.isFinite(v);
const q = (c) => `"${String(c).replace(/"/g, '""')}"`;

/** What kind of values a column holds, from the rows shown: num, text, other (arrays, objects) or none. */
function kindOf(rows, i) {
  let k = "none";
  for (const r of rows) {
    const v = r[i];
    if (v == null) continue;
    const t = isNum(v) ? "num" : typeof v === "string" || typeof v === "boolean" ? "text" : "other";
    if (k === "none") k = t; else if (k !== t) return "other";
  }
  return k;
}

/** The share of a numeric column's sum that its top rows carry: {top, share}, or null. */
const TOP = (n) => Math.min(100, Math.max(1, Math.ceil(n * 0.01)));

/** One column's profile from values in the browser. */
function profileOf(values, kind) {
  const seen = values.filter((v) => v != null);
  const p = { n: values.length, nonNull: seen.length, distinct: new Set(seen.map((v) => typeof v === "object" ? JSON.stringify(v) : v)).size };
  if (kind === "num" && seen.length) {
    p.min = Math.min(...seen); p.max = Math.max(...seen);
    const m = seen.reduce((a, b) => a + b, 0) / seen.length;
    p.sd = seen.length > 1 ? Math.sqrt(seen.reduce((a, b) => a + (b - m) ** 2, 0) / (seen.length - 1)) : null;
    p.sum = seen.reduce((a, b) => a + b, 0);
    const top = TOP(p.n);
    p.top = top; p.topSum = seen.slice().sort((a, b) => b - a).slice(0, top).reduce((a, b) => a + b, 0);
  } else if (kind === "text" && seen.length) {
    const s = seen.map(String).sort();
    p.min = s[0]; p.max = s[s.length - 1];
  }
  return p;
}

/** Every column's profile by SQL over the whole answer, or null when the server cannot run it. */
async function bySql(sql, params, cols, kinds, signal) {
  const parts = ["COUNT(*)"];
  const at = cols.map((c, i) => {
    const k = kinds[i], x = q(c), from = parts.length;
    parts.push(`COUNT(${x})`);
    if (k === "num" || k === "text") parts.push(`COUNT(DISTINCT ${x})`, `MIN(${x})`, `MAX(${x})`);
    if (k === "num") parts.push(`STDDEV(${x})`, `SUM(${x})`);
    return from;
  });
  const j = await api("/v1/query", { body: { sql: `SELECT ${parts.join(", ")} FROM (${sql}) AS answer`, params }, signal, timeout: 30000 });
  const r = j.rows?.[0];
  if (!r) return null;
  const n = r[0];
  const out = cols.map((c, i) => {
    const k = kinds[i], o = at[i];
    const p = { n, nonNull: r[o] };
    if (k === "num" || k === "text") Object.assign(p, { distinct: r[o + 1], min: r[o + 2], max: r[o + 3] });
    if (k === "num") Object.assign(p, { sd: r[o + 4], sum: r[o + 5] });
    return p;
  });
  // The top rows' share of each positive sum.
  await Promise.all(cols.map(async (c, i) => {
    const p = out[i];
    if (kinds[i] !== "num" || !(p.sum > 0) || p.nonNull < 20) return;
    p.top = TOP(p.nonNull);
    try {
      const t = await api("/v1/query", { body: { sql: `SELECT SUM(v) FROM (SELECT ${q(c)} AS v FROM (${sql}) AS answer WHERE ${q(c)} IS NOT NULL ORDER BY v DESC LIMIT ${p.top}) AS top`, params }, signal, timeout: 30000 });
      p.topSum = t.rows?.[0]?.[0];
    } catch { /* left out: no warning rather than a wrong one */ }
  }));
  return out;
}

/** The flags of one column's profile: [kind, text]. */
export function flags(p) {
  const out = [];
  if (p.n > 0 && p.nonNull === 0) out.push(["bad", "always NULL"]);
  else if (p.n > 0 && p.nonNull < p.n) out.push(["warn", `${fmt.pct(1 - p.nonNull / p.n)} NULL`]);
  if (p.nonNull > 1 && p.distinct === 1) out.push(["warn", "one value"]);
  if (p.sum > 0 && p.topSum != null && p.nonNull >= 20 && p.topSum / p.sum >= 0.5) out.push(["warn", `${fmt.plural(p.top, "row")} make ${fmt.pct(p.topSum / p.sum, 0)} of the sum`]);
  return out;
}

/** The health panel of answer `j` of statement `sql`. */
export function healthView(j, sql, params) {
  const cols = j.columns || [], rows = j.rows || [];
  if (!cols.length) return empty("table", "No columns", "The statement returned no columns to profile.");
  const body = h("div", { class: "small muted" }, "Profiling the columns…");
  const kinds = cols.map((_, i) => kindOf(rows, i));
  const dup = new Set(cols.filter((c, i) => cols.indexOf(c) !== i));
  (async () => {
    let ps = null, whole = true;
    if (sql && !dup.size) { try { ps = await bySql(sql, params, cols, kinds); } catch { ps = null; } }
    if (!ps) { whole = false; ps = cols.map((_, i) => profileOf(rows.map((r) => r[i]), kinds[i])); }
    const cellOf = (v, k) => v == null ? h("span", { class: "nul" }, "–") : k === "num" ? h("span", { class: "n" }, fmt.num(v)) : h("span", { title: String(v) }, String(v).slice(0, 40));
    body.replaceChildren(
      h("div", { class: "small muted", style: { marginBottom: "8px" } }, whole
        ? `Over every row the query matches (${fmt.plural(ps[0]?.n ?? rows.length, "row")}), by SQL.`
        : `Over the ${fmt.plural(rows.length, "row")} shown: the server could not profile the statement whole.`),
      h("table", { class: "tbl wb-health" },
        h("thead", {}, h("tr", {}, ["Column", "Values", "NULL", "Distinct", "Least", "Most", "Spread (sd)", ""].map((t) => h("th", {}, t)))),
        h("tbody", {}, cols.map((c, i) => {
          const p = ps[i], k = kinds[i];
          return h("tr", { class: p.n > 0 && p.nonNull === 0 ? "bad" : "" },
            h("td", {}, h("b", {}, c)),
            h("td", { class: "n" }, fmt.n(p.nonNull)),
            h("td", { class: "n" }, fmt.n((p.n ?? 0) - (p.nonNull ?? 0))),
            h("td", { class: "n" }, p.distinct == null ? "–" : fmt.n(p.distinct)),
            h("td", {}, cellOf(p.min, k)), h("td", {}, cellOf(p.max, k)),
            h("td", {}, k === "num" ? cellOf(p.sd, k) : "–"),
            h("td", {}, flags(p).map(([kind, t]) => h("span", { class: `dt-sc ${kind}`, style: { marginRight: "4px" } }, t))));
        }))));
  })();
  return h("div", { style: { padding: "12px 14px", overflow: "auto" } }, body);
}

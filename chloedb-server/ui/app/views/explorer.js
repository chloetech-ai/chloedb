// Log explorer: the log lines in the database's own tables (syslog, OTLP,
// an app's JSON lines), searched the way an on-call engineer does at 3 a.m.
// One table at a time: a table with a TIMESTAMP column. A line is its text
// column's words (the one it picks, or the one picked under the search
// box), with the row's other fields beside them when the table is not a
// log, when the text is empty, or when asked; a table with no text column
// (BGP updates, events) shows its fields.
// A search box (words, "a phrase", field:value, -not, level>=warn, or a raw
// WHERE ...), the time range (the top bar's, picked here too), a count of
// lines per bucket of time (time_bucket), the newest lines, the lines
// around one of them on the same host, the lines grouped by pattern, and a
// live tail. Each answer says the SQL it ran, to copy. A key limited to
// tables (the `oncall` preset's logs_*) sees its own tables only, as
// /v1/tables lists them. Without a log table, a card says how to send logs
// here: a table, an OpenTelemetry Collector's settings, a JSON lines import.
//
// It is not Logs (#/logs), which is the servers' own log lines.

import { h, s, icon, clear, fmt, card, api, every, toast, seg, toggle, copy, drawer, empty, failure, can, needs, link, sqlBlock, showTip, hideTip, debounce, store, bus, limited, grantsText, at } from "../core.js";
import { state, timeRange, setRange, setWindow } from "../data.js";
import { tables as loadTables, invalidate } from "../catalog.js";
import { quoteName } from "../sqledit.js";

export default {
  id: "explorer", title: "Log explorer", icon: "search", group: "Operate", cluster: "brief",
  keywords: "log explorer logs syslog otlp opentelemetry collector search grep tail live histogram patterns surrounding lines context host interface ip error last 15 minutes incident oncall",
  render, onParams,
};

/** A literal: 'it''s'. */
const lit = (v) => `'${String(v).replace(/'/g, "''")}'`;
const qn = (n) => quoteName(n);
const PAGE = 200;
const PATTERN_ROWS = 2000;
const TAIL_MS = 3000;
const MAX_ROWS = 2000;
const INTERVALS = { "15m": "15 minutes", "1h": "1 hour", "6h": "6 hours", "24h": "24 hours", "7d": "7 days" };

/** Severity names from the least to the worst; each name's spellings. */
const LEVELS = [["trace"], ["debug"], ["info", "informational", "notice"], ["warn", "warning"], ["error", "err"], ["critical", "crit", "fatal", "alert", "emerg", "emergency", "panic"]];
const levelOf = (v) => { const x = String(v ?? "").toLowerCase(); const i = LEVELS.findIndex((l) => l.includes(x)); return i < 0 ? null : i; };
const tone = (v) => { const i = levelOf(v); return i == null ? "" : i >= 4 ? "bad" : i === 3 ? "warn" : ""; };

/** A time of an answer (ISO text, or microseconds) as epoch ms. */
const msOf = (v) => (typeof v === "string" ? Date.parse(v) : typeof v === "number" ? v / 1000 : NaN);
const iso = (ms) => new Date(ms).toISOString();
/** A time as the literal SQL compares a TIMESTAMP with, to the microsecond the answer gave. */
const timeLit = (v) => lit(typeof v === "string" ? v : typeof v === "number" ? isoMicros(v) : String(v));
function isoMicros(us) { const ms = Math.floor(us / 1000), rest = String(Math.round(us - ms * 1000)).padStart(3, "0"); return iso(ms).replace(/Z$/, rest + "Z"); }
const clock = (ms) => { const d = new Date(ms); return d.toLocaleTimeString("en-GB", { hour12: false }) + "." + String(d.getMilliseconds()).padStart(3, "0"); };

// ---------------------------------------------------------------- tables
/** Names a log line's text goes by, the likeliest first. */
const MSG_NAMES = ["msg", "message", "body", "line", "log", "text", "raw", "content", "event"];
/** Names of the words that say what a row of another table is: a title says more than an about. */
const TEXT_NAMES = ["title", "summary", "subject", "name", "description", "detail", "question", "caption", "note"];
const TS_NAMES = ["ts", "timestamp", "time", "at", "logged_at", "created_at", "observed_ts", "event_time"];
const HOST_NAMES = ["host", "hostname", "host_name", "node", "device", "source", "server", "instance", "service"];
const SEV_NAMES = ["severity", "level", "sev", "log_level", "loglevel", "priority"];
/** Columns that are not a line's words: vectors and the like. */
const HIDDEN = new Set(["vector", "sparse", "edges"]);

/**
 * How fit a column is to be a line's text: one an index searches first (its
 * words, or a substring index), then by its name (a log's message; another
 * table's title, which says more in a line than its body), a TEXT before a
 * KEYWORD, and one made from another column (urlwords) last.
 */
function textRank(f, log) {
  const n = f.name.toLowerCase(), i = MSG_NAMES.indexOf(n), j = TEXT_NAMES.indexOf(n);
  const searched = f.substring || (f.type === "text" && f.indexed !== false);
  const named = log ? (i >= 0 ? 200 - i : j >= 0 ? 100 - j : 0) : (j >= 0 ? 200 - j : i >= 0 ? 100 - i : 0);
  return (searched ? 1000 : 0) + named + (f.type === "text" ? 60 : 0) + (f.substring ? 30 : 0) - (f.from ? 300 : 0);
}

/**
 * The columns a table's lines are read by, or null when it has no
 * TIMESTAMP: the time (`ts`), the text of a line (`msg`: a TEXT column, a
 * KEYWORD one with a substring index or a text's name, such as a title kept
 * unindexed; null when there is none, and each line is then its fields),
 * and when there are ones, a host and a
 * severity. `log` says whether it reads as a log (a level, a host, a log's
 * name): another table's lines show their other fields beside the text.
 * `texts` and `times` are the other choices, the likeliest first. `fields`
 * is the short form /v1/tables gives, `table` its name and `key` its key.
 */
export function logColumns(fields, { table = "", key = null } = {}) {
  fields = fields || [];
  const by = (types, names) => {
    const ok = fields.filter((f) => types.includes(f.type));
    for (const n of names) { const f = ok.find((x) => x.name.toLowerCase() === n); if (f) return f; }
    return null;
  };
  const times = fields.filter((f) => f.type === "timestamp");
  const ts = by(["timestamp"], TS_NAMES) || times[0];
  if (!ts) return null;
  const wordy = (f) => f.type === "text" || (f.type === "keyword" && !f.multi && (f.substring || MSG_NAMES.includes(f.name.toLowerCase()) || TEXT_NAMES.includes(f.name.toLowerCase())));
  const host = by(["keyword", "text"], HOST_NAMES);
  const sev = by(["keyword", "text", "int"], SEV_NAMES);
  const log = !!(host || sev || /^logs?_|_logs?$|syslog|otel/i.test(table));
  // Not the host or the level, which a line shows on their own.
  const texts = fields.filter((f) => wordy(f) && f !== host && f !== sev).sort((a, b) => textRank(b, log) - textRank(a, log));
  return withText({
    ts: ts.name, host: host?.name || null, sev: sev?.name || null, log, key: key && fields.some((f) => f.name === key) ? key : null, fields,
    texts: texts.map((f) => f.name), times: [ts, ...times.filter((f) => f !== ts)].map((f) => f.name),
  }, texts[0]?.name || null);
}

/** `cols` with `name` (or nothing: null) as the line's text. */
export function withText(cols, name) {
  const f = name ? cols.fields.find((x) => x.name === name) : null;
  return { ...cols, msg: f?.name || null, substring: !!f?.substring, indexed: f ? f.indexed !== false : false };
}

const blank = (v) => v == null || (typeof v === "string" ? v.trim() === "" : Array.isArray(v) ? v.length === 0 : typeof v === "object" ? Object.keys(v).length === 0 : false);

/**
 * Which text the newest lines read best by: the chosen column, or another
 * text column when the chosen one is empty in most of them and the other
 * is full in most (a story's body is empty, its title is not). Answers
 * { msg, n, empty } and, when it moved, `from` (and `empty` is the old one's).
 */
export function pickText(rows, cols) {
  const n = rows.length;
  if (!cols.msg || !n) return { msg: cols.msg, n, empty: cols.msg ? 0 : n };
  const full = (name) => rows.reduce((a, r) => a + (blank(r[name]) ? 0 : 1), 0);
  const cur = full(cols.msg);
  let best = { name: cols.msg, k: cur };
  if (cur < n / 2) for (const name of cols.texts) { if (name !== cols.msg) { const k = full(name); if (k > best.k) best = { name, k }; } }
  if (best.name !== cols.msg && best.k >= n / 2 && best.k >= 2 * cur) return { msg: best.name, from: cols.msg, n, empty: n - cur };
  return { msg: cols.msg, n, empty: n - cur };
}

/** A value as a line shows it: one line, cut at `max` characters. */
export function shortValue(v, type, max = 80) {
  let x = typeof v === "string" ? v : Array.isArray(v) && v.every((e) => e == null || typeof e !== "object") ? v.join(",") : typeof v === "object" ? JSON.stringify(v) : String(v);
  if (type === "timestamp" && typeof v === "string") x = x.replace(/\.\d+(?=Z$)/, "");
  x = x.replace(/\s+/g, " ").trim();
  return x.length > max ? x.slice(0, max - 1) + "…" : x;
}

/**
 * A row's other fields, as a line shows them: [[name, value]] in the
 * table's order, without the time, the text, the host, the level and the
 * key (shown on their own), vectors, and empty values; at most `max`.
 */
export function rowFields(r, cols, max = 10) {
  const skip = new Set([cols.ts, cols.msg, cols.host, cols.sev, cols.key]);
  const out = [];
  for (const f of cols.fields) {
    if (skip.has(f.name) || HIDDEN.has(f.type) || !(f.name in r) || blank(r[f.name])) continue;
    out.push([f.name, shortValue(r[f.name], f.type)]);
  }
  return out.length > max ? [...out.slice(0, max), ["…", `+${out.length - max} more`]] : out;
}

/** A line's words, for its pattern: the text, or its fields when it has none (not its key, which is each row's own). */
export function lineText(r, cols) {
  const t = cols.msg ? r[cols.msg] : null;
  return blank(t) ? rowFields(r, cols, 99).map(([k, v]) => `${k}=${v}`).join(" ") : String(t);
}

// ---------------------------------------------------------------- the search
/**
 * The search box as SQL conditions: words and "phrases" find the line's
 * text (ILIKE '%...%' on a substring index or a column with no index,
 * MATCH otherwise; with no text column, a field's exact value), field:value
 * a column's value (a * in it is LIKE's %), field>5, -term its negation,
 * level>=warn the severities at or above warn, and `WHERE ...` a condition
 * as written. Answers { conds: [sql], terms: [text to highlight] }.
 */
export function searchSql(text, cols, mode = "auto") {
  text = String(text || "").trim();
  if (!text) return { conds: [], terms: [] };
  if (/^where\s/i.test(text)) return { conds: [`(${text.replace(/^where\s+/i, "")})`], terms: [], raw: true };
  // MATCH needs a TEXT column with its index; any other is read with ILIKE.
  const mf = cols.msg ? cols.fields.find((f) => f.name === cols.msg) : null;
  const words = !!mf && mf.type === "text" && mf.indexed !== false && (mode === "words" || (mode === "auto" && !cols.substring));
  // No text column: a word is one of the fields' values (KEYWORD, and INT for a number).
  const anyField = (t) => {
    const num = /^-?\d+$/.test(t);
    const one = cols.fields.filter((f) => !f.multi && f.indexed !== false && (f.type === "keyword" || (num && f.type === "int")));
    return one.length ? `(${one.map((f) => `${qn(f.name)} = ${f.type === "int" ? t : lit(t)}`).join(" OR ")})` : "1 = 0";
  };
  const ALIAS = { level: cols.sev, severity: cols.sev, sev: cols.sev, host: cols.host, msg: cols.msg, message: cols.msg };
  const field = (name) => {
    const n = (cols.fields.some((x) => x.name.toLowerCase() === name.toLowerCase()) ? name : ALIAS[name.toLowerCase()] || name).toLowerCase();
    const f = cols.fields.find((x) => x.name.toLowerCase() === n);
    if (f) return { name: f.name, type: f.type, substring: !!f.substring, indexed: f.indexed !== false };
    // A path of a JSON column: attributes.iface.
    const root = n.split(".")[0], jf = cols.fields.find((x) => x.name.toLowerCase() === root && x.type === "json");
    return jf && n.includes(".") ? { name: jf.name + name.slice(root.length), type: "path", path: true } : null;
  };
  const conds = [], terms = [];
  const re = /(-?)(?:([A-Za-z_][\w.]*)(:|!=|>=|<=|>|<|=))?("(?:[^"\\]|\\.)*"|\S+)/g;
  let m;
  while ((m = re.exec(text))) {
    const [, neg, name, op, raw] = m;
    const quoted = raw.startsWith('"') && raw.endsWith('"') && raw.length > 1;
    const value = quoted ? raw.slice(1, -1).replace(/\\(.)/g, "$1") : raw;
    let c = null;
    const f = name ? field(name) : null;
    if (f) {
      const col = f.path ? f.name.split(".").map(qn).join(".") : qn(f.name);
      const num = /^-?\d+(\.\d+)?$/.test(value) && !quoted;
      const val = num && (f.type === "int" || f.type === "float" || f.type === "path") ? value : lit(value);
      const isSev = cols.sev && f.name === cols.sev && f.type !== "int" && levelOf(value) != null && /[<>]/.test(op);
      if (isSev) {
        // The levels at or above (or below) it, as the table spells them
        // (`sevValues`, its values), or in the usual spellings.
        const i = levelOf(value);
        const want = (x) => { const j = levelOf(x); return j != null && (op === ">=" ? j >= i : op === ">" ? j > i : op === "<=" ? j <= i : j < i); };
        const spell = cols.sevValues?.length ? cols.sevValues.filter(want) : [...new Set(LEVELS.flat().filter(want).flatMap((x) => [x, x.toUpperCase()]))];
        c = `${col} IN (${(spell.length ? spell : [value]).map(lit).join(", ")})`;
      } else if (op === ":" || op === "=") {
        if (f.type === "text") c = f.substring || !f.indexed ? `${col} ILIKE ${lit("%" + value + "%")}` : `MATCH(${col}, ${lit(quoted ? `"${value}"` : value)})`;
        else if (value.includes("*") && (f.type === "keyword" || f.type === "path")) c = `${col} ${f.substring && value.replace(/\*+$/, "").includes("*") ? "ILIKE" : "LIKE"} ${lit(value.replace(/\*/g, "%"))}`;
        else c = `${col} = ${val}`;
      } else c = `${col} ${op} ${val}`;
      if (!neg && !isSev && (f.name === cols.msg || op === ":" || op === "=") && (f.type === "text" || f.type === "keyword")) terms.push(value.replace(/\*/g, "%"));
    } else {
      const whole = name ? m[0].slice(neg.length) : value;
      const t = name ? whole : value;
      c = !cols.msg ? anyField(t) : words ? `MATCH(${qn(cols.msg)}, ${lit(quoted ? `"${t}"` : t)})` : `${qn(cols.msg)} ILIKE ${lit("%" + t + "%")}`;
      if (!neg) terms.push(t);
    }
    conds.push(neg ? `NOT (${c})` : c);
  }
  return { conds, terms };
}

// ---------------------------------------------------------------- patterns
/**
 * A line as its pattern: what changes from one line of a kind to the next
 * (numbers, addresses, ids, times, quoted text) becomes a placeholder, so
 * "Interface Gi0/1 down on 10.0.12.2" and "... Gi0/3 down on 10.0.13.9"
 * are both "Interface Gi<n>/<n> down on <ip>".
 */
export function logPattern(text) {
  // A syslog mnemonic (%LINK-3-UPDOWN) names the kind of line, its level included: kept.
  return String(text ?? "").split(/(%[A-Z][A-Z0-9_]*-\d-[A-Z0-9_]+)/).map((x, i) => (i % 2 ? x : variable(x))).join("").replace(/\s+/g, " ").trim().slice(0, 300);
}
function variable(text) {
  return text
    .replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, "<uuid>")
    .replace(/\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b/gi, "<mac>")
    .replace(/\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?(?:\/\d{1,2})?\b/g, "<ip>")
    .replace(/\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2}(?:[.,]\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?/g, "<time>")
    .replace(/\b\d{1,2}:\d{2}:\d{2}(?:[.,]\d+)?\b/g, "<time>")
    .replace(/\b(?:[0-9a-f]{1,4}:){2,7}[0-9a-f]{0,4}\b/gi, (x) => (/[a-f]/i.test(x) || x.split(":").length > 4 ? "<ip>" : x))
    .replace(/\b0x[0-9a-f]+\b/gi, "<hex>")
    .replace(/\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{8,}\b/gi, "<hex>")
    .replace(/'(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*"/g, "<str>")
    .replace(/\d+(?:\.\d+)?/g, "<n>");
}

/** Lines grouped by pattern, the most first: [{pattern, n, hosts, first, last, sample, rows}]. `msgKey` is a column, or a function of a row. */
export function groupPatterns(rows, msgKey, tsKey, hostKey) {
  const by = new Map();
  const text = typeof msgKey === "function" ? msgKey : (r) => r[msgKey];
  for (const r of rows) {
    const p = logPattern(text(r));
    let g = by.get(p);
    if (!g) by.set(p, (g = { pattern: p, n: 0, hosts: new Set(), first: Infinity, last: -Infinity, sample: text(r), rows: [] }));
    g.n++; g.rows.push(r);
    if (hostKey && r[hostKey] != null) g.hosts.add(r[hostKey]);
    const t = msOf(r[tsKey]);
    if (t < g.first) g.first = t;
    if (t > g.last) g.last = t;
  }
  return [...by.values()].sort((a, b) => b.n - a.n || b.last - a.last);
}

// ---------------------------------------------------------------- sending logs here
/** The CREATE TABLE of a log table: OTLP's column names, so the Collector and JSON lines both fill it. */
export const tableSql = (name = "logs_syslog") => `CREATE TABLE ${name} (
  ts TIMESTAMP NOT NULL,
  severity KEYWORD,
  host KEYWORD OPTIONS(substring = true),
  service KEYWORD,
  body TEXT OPTIONS(substring = true),
  attributes JSON
) OPTIONS(retain = '30 days', compression = 'zstd')`;

/** An OpenTelemetry Collector's settings that send syslog (and any logs it gets) to this server's /v1/logs. */
export const collectorYaml = (base, table = "logs_syslog") => `receivers:
  syslog:
    udp:
      listen_address: 0.0.0.0:5514
    protocol: rfc3164
  otlp:
    protocols:
      grpc:
      http:

processors:
  batch:

exporters:
  otlphttp/chloedb:
    logs_endpoint: ${base}/v1/logs?table=${table}
    encoding: json
    headers:
      Authorization: Bearer \${env:CHLOEDB_SHIP_KEY}

service:
  pipelines:
    logs:
      receivers: [syslog, otlp]
      processors: [batch]
      exporters: [otlphttp/chloedb]`;

/** A JSON lines import of the same table, for a file or a script. */
export const importCurl = (base, table = "logs_syslog") => `printf '%s\\n' \\
  '{"ts":"2026-11-13T03:00:01Z","severity":"error","host":"core-rtr-02","body":"%LINK-3-UPDOWN: Interface GigabitEthernet0/1, changed state to down"}' \\
  '{"ts":"2026-11-13T03:00:04Z","severity":"warn","host":"core-rtr-02","body":"%BGP-5-ADJCHANGE: neighbor 10.0.12.2 Down BGP Notification sent"}' \\
| curl -s -X POST "${base}/v1/import?table=${table}&format=jsonl" \\
    -H "Authorization: Bearer $CHLOEDB_SHIP_KEY" -H 'content-type: application/x-ndjson' --data-binary @-`;

/** The body that makes the shipper's key: a grant of append (MARCO-403), which inserts new lines and reads nothing. */
export const shipperKey = (table = "logs_syslog") => ({ name: `${table.replace(/_/g, "-")}-collector`, tables: { [table]: "append" } });
const keyCurl = (base, table) => `curl -s -X POST ${base}/v1/admin/keys -H "Authorization: Bearer $ADMIN_KEY" \\
  -H 'content-type: application/json' \\
  -d '${JSON.stringify(shipperKey(table))}'`;

function serverBase() { const b = at("/v1/sql").replace(/\/v1\/sql$/, ""); return /^https?:/.test(b) ? b : location.origin + b; }
/** Whether the signed-in key, limited to tables, is admin of table `name` (a grant of it, or of a prefix* of it). */
function ownsTable(name) {
  const n = String(name).toLowerCase();
  return Object.entries(state.whoami?.tables || {}).some(([g, sc]) => sc === "admin" && (g === "*" || g.toLowerCase() === n || (g.endsWith("*") && n.startsWith(g.slice(0, -1).toLowerCase()))));
}

function sendLogsCard({ onMade } = {}) {
  const base = serverBase();
  const nameIn = h("input", { class: "input sm", value: "logs_syslog", "aria-label": "Table name", style: { width: "160px" } });
  const name = () => (/^[A-Za-z_]\w*$/.test(nameIn.value.trim()) ? nameIn.value.trim() : "logs_syslog");
  const blocks = h("div", { class: "stack" });
  let secret = null;
  const draw = () => clear(blocks,
    step("1", "A table for the lines", "Columns named as OTLP names them, so the Collector and JSON lines both fill it. The substring indexes answer LIKE and ILIKE '%Gi0/1%' anywhere in a line, an IP or a host name; retain drops lines after 30 days.", tableSql(name()), makeBtn()),
    step("2", "A key that only appends them", `The shipper's key has a grant of append on ${name()}: it inserts new lines (OTLP, imports, INSERT) and reads nothing, not even these lines, nor alerts, so the copy in every host's config cannot read user ids or tokens out of the logs, or quiet the pages. A key that writes a table reads it too: give the people on call an oncall key (Access and audit) to search. ${secret ? "Here it is, once: keep it in the Collector's environment as CHLOEDB_SHIP_KEY." : "An admin makes it (Access and audit, or this call), and sees its secret once."}`, secret ? `export CHLOEDB_SHIP_KEY=${secret}` : keyCurl(base, name()), keyBtn(), secret ? "secret" : null),
    step("3", "Send them: an OpenTelemetry Collector", "Syslog on UDP 5514 and anything that speaks OTLP, batched and sent to this server's /v1/logs (OTLP/HTTP, JSON). Run it with otelcol-contrib --config collector.yaml.", collectorYaml(base, name()), null, "yaml"),
    step("…", "or a JSON lines import", "From a file, a script or a cron job: one object a line, the same columns.", importCurl(base, name())));
  function makeBtn() {
    const b = h("button", { class: "btn sm primary", onclick: async () => {
      b.disabled = true;
      try { await api("/v1/sql", { body: { sql: tableSql(name()) }, reason: "a log table from the Log explorer" }); invalidate(); toast(`Table ${name()} made`, "ok"); onMade?.(name()); }
      catch (e) { toast(e.message, "bad"); b.disabled = false; }
    } }, icon("plus"), "Make this table");
    if (!can("admin") && !ownsTable(name())) { b.disabled = true; b.title = limited() ? `needs a key that is admin of ${name()} (this one: ${grantsText(state.whoami?.tables)})` : needs("admin"); }
    return b;
  }
  function keyBtn() {
    if (secret) return null;
    const b = h("button", { class: "btn sm", onclick: async () => {
      b.disabled = true;
      try { const j = await api("/v1/admin/keys", { body: shipperKey(name()), reason: "a log shipper's key, from the Log explorer" }); secret = j.secret || null; draw(); toast("Key made: its secret is shown once", "ok"); }
      catch (e) { toast(e.message, "bad"); b.disabled = false; }
    } }, icon("key"), "Make the shipper's key");
    if (!can("admin")) { b.disabled = true; b.title = needs("admin"); }
    return b;
  }
  nameIn.oninput = debounce(draw, 250);
  draw();
  return card({
    title: "Send your logs here", sub: "no table of log lines yet" + (limited() ? ` that this key reads (${grantsText(state.whoami?.tables)})` : ""),
    tools: [h("label", { class: "small dim row nw", style: { gap: "6px" } }, "table", nameIn)],
    body: [h("p", { class: "dim", style: { marginTop: 0 } }, "A log table is any table with a TIMESTAMP and a text column. Make one, send lines to it, and search them here: the last 15 minutes' errors, a host per minute, an interface flapping, an IP."), blocks],
  });
}

function step(n, title, text, code, action, lang) {
  const secret = lang === "secret";
  return h("div", { class: "lx-step" },
    h("div", { class: "lx-step-n" }, n),
    h("div", { class: "grow", style: { minWidth: 0 } },
      h("div", { class: "row between" }, h("b", {}, title), h("div", { class: "row nw", style: { gap: "6px" } }, action, h("button", { class: "btn sm ghost", onclick: () => copy(code) }, icon("copy"), "Copy"))),
      h("div", { class: "small dim", style: { margin: "2px 0 6px" } }, text),
      lang === "yaml" || secret ? h("pre", { class: "code lx-code", "data-secret": secret ? "key" : null }, code) : sqlBlock(code, { class: "code lx-code" })));
}

// ---------------------------------------------------------------- the page
let reparams = null;
function onParams(r) { reparams?.(r); }

function render(page, route) {
  const head = h("div", { class: "page-head" });
  const body = h("div", { class: "stack" });
  page.append(head, body);
  let alive = true;
  const st = {
    tables: [], table: route.params.get("table") || store.get("explorer.table", null), cols: null,
    q: route.params.get("q") || "", mode: store.get("explorer.mode", "auto"),
    rows: [], hist: null, patterns: null, pattern: null, tab: route.params.get("tab") || "lines",
    live: store.get("explorer.live", true), more: true, ran: new Map(), err: null,
    compat: { interval: true, bucket: true }, seen: new Set(),
    // What the newest lines said of the text column (pickText), and whether it was looked at.
    text: null, looked: new Set(),
  };
  const stops = [];
  const off = [bus.on("range", () => { if (alive && st.cols) { syncRange(); reload(); } }), bus.on("token", () => { invalidate(); boot(); })];

  // The top of the page: the table, the search, the range, live.
  const tableSel = h("select", { class: "select", "aria-label": "Log table", onchange: () => pickTable(tableSel.value) });
  const qIn = h("input", { class: "input lx-q", type: "search", placeholder: 'Search: Gi0/1  "hold time"  host:core-rtr-02  level>=warn  -debug   (or WHERE ...)', value: st.q, "aria-label": "Search the lines", spellcheck: "false", autocomplete: "off" });
  qIn.onkeydown = (e) => { if (e.key === "Enter") { st.q = qIn.value; writeUrl(); reload(); } };
  qIn.oninput = debounce(() => { if (qIn.value !== st.q) { st.q = qIn.value; writeUrl(); reload(); } }, 600);
  const modeSeg = seg([["auto", "Auto", "Substring where the column has a substring index, words otherwise"], ["substring", "Substring", "ILIKE '%text%': any part of a line, an IP, an interface name (fast on a substring index)"], ["words", "Words", "MATCH: whole words, ranked text search"]], st.mode, (v) => { st.mode = v; store.set("explorer.mode", v); reload(); });
  const RANGES = [["15m", "15 min"], ["1h", "1 h"], ["24h", "24 h"], ["7d", "7 d"], ["custom", "Custom"]];
  const rangeSeg = seg(RANGES.map(([k, l]) => [k, l]), currentRange(), (v) => { if (v === "custom") { showCustom(true); fillCustom(); } else { showCustom(false); setRange(v); } });
  const fromIn = h("input", { class: "input sm", type: "datetime-local", step: "1", "aria-label": "From" });
  const toIn = h("input", { class: "input sm", type: "datetime-local", step: "1", "aria-label": "To" });
  const customBox = h("span", { class: "row nw", style: { gap: "6px", display: currentRange() === "custom" ? "" : "none" } }, fromIn, h("span", { class: "dim" }, "to"), toIn,
    h("button", { class: "btn sm", onclick: () => { const a = Date.parse(fromIn.value), b = Date.parse(toIn.value); if (a > 0 && b > a) setWindow(a, b); else toast("Pick a start before the end", "warn"); } }, "Apply"));
  const liveBtn = h("button", { class: "btn sm live", onclick: () => { st.live = !st.live; store.set("explorer.live", st.live); drawLive(); if (st.live) tail(); } });
  const sendBtn = h("button", { class: "btn sm ghost", onclick: () => { const d = drawer({ title: "Send your logs here", wide: true, body: sendLogsCard({ onMade: (n) => { d.close(); boot(n); } }) }); } }, icon("download"), "Send logs here");

  function showCustom(on) { customBox.style.display = on ? "" : "none"; }
  function currentRange() { const r = timeRange(); return r.fixed ? "custom" : r.range || "1h"; }
  function fillCustom() { const r = timeRange(); const loc = (ms) => { const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60e3); return d.toISOString().slice(0, 19); }; fromIn.value = loc(r.from); toIn.value = loc(r.to); }
  function syncRange() { const v = currentRange(); rangeSeg.set(v); showCustom(v === "custom"); if (v === "custom") fillCustom(); drawLive(); }
  function canTail() { return !timeRange().fixed; }
  function drawLive() {
    const on = st.live && canTail();
    clear(liveBtn, icon(on ? "pause" : "play"), on ? "Live" : canTail() ? "Paused" : "Live (a fixed window)");
    liveBtn.classList.toggle("paused", !on);
    liveBtn.disabled = !canTail();
    liveBtn.title = canTail() ? (on ? `New lines every ${TAIL_MS / 1000} s; click to pause` : "Follow new lines") : "A fixed window does not grow: pick a range up to now to follow new lines";
  }

  // The parts below.
  const histBox = h("div", { class: "lx-hist" });
  const histCard = card({ title: "Lines over time", sub: "…", body: histBox, cls: "lx-histcard" });
  const tabs = h("div", { class: "tabs", role: "tablist" });
  const main = h("div");
  const sqlBox = h("div", { class: "stack" });
  const sqlCard = card({ title: "The SQL it ran", sub: "copy it, or open it in the SQL workbench", body: sqlBox });
  const errBox = h("div");

  function writeUrl() {
    const q = new URLSearchParams();
    if (st.table) q.set("table", st.table);
    if (st.q) q.set("q", st.q);
    if (st.tab !== "lines") q.set("tab", st.tab);
    const want = link("explorer", Object.fromEntries(q));
    if (location.hash !== want) history.replaceState(null, "", want);
  }

  reparams = (r) => {
    const t = r.params.get("table"), q = r.params.get("q") ?? "";
    let changed = false;
    if (t && t !== st.table) { pickTable(t); return; }
    if (q !== st.q) { st.q = q; qIn.value = q; changed = true; }
    if (changed) reload();
  };

  async function boot(want) {
    clear(body, h("div", { class: "dim small" }, "Reading the tables…"));
    try { st.tables = await loadTables(true); }
    catch (e) { clear(head); clear(body, failure(e, "The tables")); return; }
    if (!alive) return;
    const logs = st.tables.map((t) => ({ name: t.table, cols: logColumns(t.short?.fields, { table: t.table, key: t.short?.key }) })).filter((t) => t.cols);
    st.logs = logs;
    if (want) st.table = want;
    if (!logs.some((t) => t.name === st.table)) st.table = (logs.find((t) => /^logs?_|_logs?$|syslog|otel/i.test(t.name)) || logs.find((t) => t.cols.log && t.cols.msg) || logs.find((t) => t.cols.msg) || logs[0])?.name || null;
    clear(head, h("div", {}, h("h1", {}, "Log explorer"), h("p", {}, "The log lines in this database's tables: search, a time range, the lines around one, patterns, a live tail. The servers' own lines are in ", h("a", { href: link("logs") }, "Logs"), ".")),
      h("div", { class: "tools" }, logs.length ? tableSel : null, sendBtn));
    if (!logs.length) { clear(body, sendLogsCard({ onMade: (n) => boot(n) })); return; }
    clear(tableSel, logs.map((t) => h("option", { value: t.name, selected: t.name === st.table }, t.name)));
    clear(body,
      h("div", { class: "lx-bar" }, qIn, h("div", { class: "row nw lx-ctl" }, modeSeg, rangeSeg, customBox, liveBtn)),
      hintBox,
      errBox, histCard, h("div", { class: "card lx-main" }, h("div", { class: "hd" }, tabs), h("div", { class: "bd flush" }, main)), sqlCard);
    pickTable(st.table, true);
  }

  // Under the search box: what a line is (its text, its time, its other fields), each changeable, and what a search does.
  const hintBox = h("div", { class: "small dim lx-hint" });
  /** The columns `t` is read by: its own, or the ones picked for it here (kept in the browser). */
  function colsOf(t) {
    const pick = store.get("explorer.cols." + t.name, null) || {};
    let c = t.cols;
    if (pick.msg !== undefined && (pick.msg === null || c.texts.includes(pick.msg))) c = { ...withText(c, pick.msg), picked: true };
    if (pick.ts && c.times.includes(pick.ts)) c = { ...c, ts: pick.ts };
    if (typeof pick.fields === "boolean") c = { ...c, showFields: pick.fields };
    return c;
  }
  function choose(what, v) {
    const k = "explorer.cols." + st.table, pick = store.get(k, null) || {};
    pick[what] = v;
    store.set(k, pick);
    const t = st.logs.find((x) => x.name === st.table);
    st.cols = colsOf(t);
    st.text = null;
    drawHint();
    if (what === "fields") drawMain(); else reload();
  }
  const fieldsShown = () => st.cols.showFields ?? !st.cols.log;
  function drawHint() {
    const c = st.cols;
    if (!c) return clear(hintBox);
    const sel = (label, value, options, onchange) => h("select", { class: "select sm", "aria-label": label, onchange: (e) => onchange(e.target.value) }, options.map(([v, l]) => h("option", { value: v, selected: v === value }, l)));
    const textSel = sel("The text of a line", c.msg ?? "", [...c.texts.map((n) => [n, n]), ["", c.texts.length ? "none: the fields" : "no text column: the fields"]], (v) => choose("msg", v || null));
    const timeSel = c.times.length > 1 ? sel("The time of a line", c.ts, c.times.map((n) => [n, n]), (v) => choose("ts", v)) : h("b", {}, c.ts);
    const fieldsSw = toggle(fieldsShown(), (on) => choose("fields", on), "other fields");
    const T = st.text;
    const notes = [];
    if (T?.from) notes.push(`${T.from} is empty in ${fmt.n(T.empty)} of the newest ${fmt.n(T.n)} lines, so they show ${c.msg}.`);
    else if (c.msg && T && T.empty > T.n / 2) notes.push(`${c.msg} is empty in ${fmt.n(T.empty)} of the newest ${fmt.n(T.n)} lines: those show their other fields.`);
    if (c.host || c.sev) notes.push([c.host ? `From ${c.host}` : "", c.sev ? `${c.host ? ", at" : "At"} the level ${c.sev} (search ${c.sev}>=warn)` : "", "."].join(""));
    if (!c.msg) notes.push("A word finds a field's exact value, field:value searches one field, WHERE ... takes SQL.");
    else if (c.substring) notes.push(`${c.msg} has a substring index: any part of a line is found from it.`);
    else if (c.indexed && c.fields.find((f) => f.name === c.msg)?.type === "text") notes.push(`${c.msg} has no substring index: words are found with MATCH, and Substring reads every line in the range.`);
    else notes.push(`${c.msg} has no index of its words: a search reads every line in the range.`);
    clear(hintBox, h("div", { class: "row lx-pick" }, h("span", { class: "row nw" }, "Lines: text ", textSel), h("span", { class: "row nw" }, "time ", timeSel), fieldsSw), h("div", {}, notes.join(" ")));
  }

  function pickTable(name, first = false) {
    const t = st.logs?.find((x) => x.name === name) || st.logs?.[0];
    if (!t) return;
    st.table = t.name; st.cols = colsOf(t); st.text = null; store.set("explorer.table", t.name);
    tableSel.value = t.name;
    drawHint();
    if (!first) st.q = qIn.value;
    writeUrl(); syncRange();
    // The levels as the table spells them, for level>=warn.
    const sevOf = t.cols.sev && t.cols.sevValues === undefined && t.cols.fields.find((f) => f.name === t.cols.sev)?.type !== "int"
      ? api("/v1/sql", { body: { sql: `SELECT ${qn(t.cols.sev)}, count(*) FROM ${qn(t.name)} GROUP BY ${qn(t.cols.sev)} LIMIT 100` } }).then((j) => { t.cols.sevValues = j.rows.map((r) => r[0]).filter((v) => typeof v === "string"); }, () => { t.cols.sevValues = null; })
      : Promise.resolve();
    sevOf.then(() => { if (st.table === t.name) st.cols.sevValues = t.cols.sevValues; });
    sevOf.then(() => { if (alive && st.table === t.name) reload(); });
  }

  // ---- SQL
  function timeCond() {
    const c = qn(st.cols.ts), r = timeRange();
    if (!r.fixed && r.range && st.compat.interval) return `${c} > now() - INTERVAL '${INTERVALS[r.range] || `${Math.round((r.to - r.from) / 60e3)} minutes`}'`;
    if (!r.fixed) return `${c} > ${lit(iso(r.from))}`;
    return `${c} >= ${lit(iso(r.from))} AND ${c} < ${lit(iso(r.to))}`;
  }
  function where(extra = []) {
    const s = searchSql(st.q, st.cols, st.mode);
    st.terms = s.terms;
    return [timeCond(), ...s.conds, ...extra].join("\n  AND ");
  }
  const T = () => qn(st.table);
  function bucketOf() {
    const r = timeRange(), span = r.to - r.from;
    const w = span <= 2 * 3600e3 ? [60e3, "1 minute", "minute"] : span <= 6 * 3600e3 ? [5 * 60e3, "5 minutes", "minute"] : span <= 26 * 3600e3 ? [15 * 60e3, "15 minutes", "hour"] : span <= 8 * 86400e3 ? [2 * 3600e3, "2 hours", "hour"] : [86400e3, "1 day", "day"];
    if (!st.compat.bucket) { const unit = w[2], ms = { minute: 60e3, hour: 3600e3, day: 86400e3 }[unit]; return { ms, label: "1 " + unit, sql: `date_trunc('${unit}', ${qn(st.cols.ts)})` }; }
    return { ms: w[0], label: w[1], sql: `time_bucket('${w[1]}', ${qn(st.cols.ts)})` };
  }
  /** Runs `sql`, kept under `what` for "The SQL it ran"; older servers' refusals of INTERVAL and time_bucket retried without them. */
  async function run(what, make, opts = {}) {
    for (let tries = 0; ; tries++) {
      const sql = make();
      const t0 = performance.now();
      try {
        const j = await api("/v1/sql", { body: { sql }, ...opts });
        st.ran.set(what, { sql, ms: j.took_ms ?? performance.now() - t0, n: (j.rows || []).length });
        drawSql();
        return j;
      } catch (e) {
        const m = String(e.message || "");
        if (tries < 2 && sql.includes("INTERVAL") && /INTERVAL/.test(m) && /not a value|not supported|unsupported/i.test(m)) { st.compat.interval = false; continue; }
        if (tries < 2 && sql.includes("time_bucket(") && /time_bucket/.test(m)) { st.compat.bucket = false; continue; }
        st.ran.set(what, { sql, error: m });
        drawSql();
        throw e;
      }
    }
  }
  const objects = (j) => (j.rows || []).map((r) => Object.fromEntries(j.columns.map((c, i) => [c, r[i]])));
  const keyOf = (r) => JSON.stringify(r);

  // ---- loading
  let gen = 0;
  async function reload() {
    if (!st.cols) return;
    const my = ++gen;
    st.pattern = null; st.patterns = null; st.more = true;
    clear(errBox);
    drawTabs();
    clear(main, h("div", { class: "dim small", style: { padding: "12px" } }, "Searching…"));
    const lines = run("Lines", () => `SELECT * FROM ${T()}\nWHERE ${where()}\nORDER BY ${qn(st.cols.ts)} DESC\nLIMIT ${PAGE}`);
    const hist = loadHist();
    try {
      const j = await lines;
      if (my !== gen || !alive) return;
      st.rows = objects(j); st.seen = new Set(st.rows.map(keyOf)); st.more = st.rows.length >= PAGE;
      lookAtText();
      drawMain();
    } catch (e) { if (my === gen) { clear(main); clear(errBox, failureOf(e)); } }
    await hist.catch(() => {});
    if (st.tab === "patterns") loadPatterns();
  }
  /**
   * Once a table, on its newest lines (with no search): when the text
   * column is empty in most of them and another is full, the lines show
   * that one (the hint says so); a column picked here stays.
   */
  function lookAtText() {
    if (!st.cols || st.q || !st.rows.length) return;
    const t = st.logs.find((x) => x.name === st.table);
    if (st.cols.picked) { st.text = pickText(st.rows, { ...st.cols, texts: [] }); drawHint(); return; }
    if (st.looked.has(st.table)) { if (!st.text) { st.text = t.text || null; drawHint(); } return; }
    st.looked.add(st.table);
    const d = pickText(st.rows, st.cols);
    if (d.from) { t.cols = withText(t.cols, d.msg); st.cols = colsOf(t); }
    st.text = t.text = d;
    drawHint();
  }
  function failureOf(e) {
    if (e?.status === 403 || e?.status === 401) return failure(e, `Table ${st.table}`);
    return h("div", { class: "error" }, h("b", {}, "The search failed: "), e.message, h("div", { class: "small", style: { marginTop: "4px" } }, "A search of a column is field:value, a level is level>=warn, and WHERE ... takes a SQL condition as written."));
  }
  async function loadHist() {
    const sev = st.cols.sev ? `, ${qn(st.cols.sev)}` : "";
    try {
      const j = await run("Lines over time", () => { const bb = bucketOf(); return `SELECT ${bb.sql} AS t${sev}, count(*) AS n\nFROM ${T()}\nWHERE ${where()}\nGROUP BY t${sev}\nLIMIT 20000`; });
      if (!alive) return;
      st.hist = { bucket: bucketOf(), rows: j.rows.map((r) => ({ t: msOf(r[0]), sev: st.cols.sev ? r[1] : null, n: r[st.cols.sev ? 2 : 1] })) };
      drawHist();
    } catch (e) { clear(histBox, h("div", { class: "small dim" }, "No histogram: " + e.message)); }
  }
  async function loadMore() {
    const oldest = st.rows[st.rows.length - 1];
    if (!oldest) return;
    const j = await run("Older lines", () => `SELECT * FROM ${T()}\nWHERE ${where([`${qn(st.cols.ts)} <= ${timeLit(oldest[st.cols.ts])}`])}\nORDER BY ${qn(st.cols.ts)} DESC\nLIMIT ${PAGE + 50}`);
    const add = objects(j).filter((r) => !st.seen.has(keyOf(r)));
    for (const r of add) st.seen.add(keyOf(r));
    st.rows.push(...add);
    st.more = j.rows.length >= PAGE + 50 && add.length > 0;
    drawMain();
  }
  async function loadPatterns() {
    const my = gen;
    clear(main, h("div", { class: "dim small", style: { padding: "12px" } }, "Grouping the lines…"));
    try {
      // The text's lines need only it; a table with no text column (or lines whose text is empty) is grouped by its fields.
      const c = st.cols;
      const cols = c.msg && !fieldsShown() ? [c.ts, c.msg, c.host, c.sev].filter(Boolean).map(qn).join(", ") : "*";
      const j = await run("Patterns (the lines)", () => `SELECT ${cols} FROM ${T()}\nWHERE ${where()}\nORDER BY ${qn(c.ts)} DESC\nLIMIT ${PATTERN_ROWS}`);
      if (my !== gen || !alive) return;
      const rows = objects(j);
      st.patterns = { rows: rows.length, groups: groupPatterns(rows, (r) => lineText(r, c), c.ts, c.host) };
      if (st.tab === "patterns") drawMain();
    } catch (e) { clear(main); clear(errBox, failureOf(e)); }
  }

  // ---- live tail
  let tailing = false;
  async function tail() {
    if (!alive || !st.cols || !st.live || !canTail() || tailing) return;
    const newest = st.rows[0];
    tailing = true;
    try {
      const since = newest ? [`${qn(st.cols.ts)} >= ${timeLit(newest[st.cols.ts])}`] : [];
      const j = await run("Live tail", () => `SELECT * FROM ${T()}\nWHERE ${where(since)}\nORDER BY ${qn(st.cols.ts)} DESC\nLIMIT ${PAGE}`);
      const fresh = objects(j).filter((r) => !st.seen.has(keyOf(r)));
      if (fresh.length) {
        for (const r of fresh) st.seen.add(keyOf(r));
        st.rows = [...fresh, ...st.rows].slice(0, MAX_ROWS);
        st.newKeys = new Set(fresh.map(keyOf));
        if (st.tab === "lines" && !st.pattern) drawMain();
        loadHist();
      }
    } catch { /* the next tick tries again */ }
    tailing = false;
  }
  stops.push(every(TAIL_MS, tail, false, { always: true }));

  // ---- drawing
  function drawTabs() {
    const n = { lines: st.rows.length ? fmt.n(st.rows.length) + (st.more ? "+" : "") : null, patterns: st.patterns ? fmt.n(st.patterns.groups.length) : null };
    clear(tabs, [["lines", "Lines"], ["patterns", "Patterns"]].map(([k, v]) => h("button", { role: "tab", "aria-selected": String(k === st.tab), onclick: () => { st.tab = k; writeUrl(); drawTabs(); if (k === "patterns" && !st.patterns) loadPatterns(); else drawMain(); } }, v, n[k] != null ? h("span", { class: "n" }, n[k]) : null)));
  }

  function drawMain() {
    drawTabs();
    if (st.tab === "patterns") return drawPatterns();
    const rows = st.pattern ? st.pattern.rows : st.rows;
    const top = st.pattern ? h("div", { class: "lx-chipbar" }, h("span", { class: "chip" }, "pattern: ", h("code", {}, st.pattern.pattern), h("button", { class: "btn xs ghost", "aria-label": "Show every line", onclick: () => { st.pattern = null; drawMain(); } }, icon("x"))), h("span", { class: "small dim" }, `${fmt.n(rows.length)} of the newest ${fmt.n(st.patterns?.rows || 0)} lines`)) : null;
    if (!rows.length) {
      const others = st.pattern ? [] : st.cols.times.filter((x) => x !== st.cols.ts);
      clear(main, top, empty("search", "No lines", `No line of ${st.table} matches in ${timeRange().label} (by its ${st.cols.ts}). Widen the range, or search less${others.length ? `, or pick another time under the search box (${others.join(", ")})` : ""}.`));
      return;
    }
    const list = h("div", { class: "lx-lines", role: "list" });
    const nw = st.newKeys || new Set();
    for (const r of rows) {
      const lv = st.cols.sev ? r[st.cols.sev] : null;
      list.append(h("div", { class: `lx-line ${tone(lv)}${nw.has(keyOf(r)) ? " lx-new" : ""}`, role: "listitem", tabIndex: 0, title: "Show the lines around it",
        onclick: () => around(r), onkeydown: (e) => { if (e.key === "Enter") around(r); } }, lineParts(r, st.cols)));
    }
    st.newKeys = null;
    const moreBtn = !st.pattern && st.more ? h("button", { class: "btn sm", onclick: async () => { moreBtn.disabled = true; try { await loadMore(); } catch (e) { toast(e.message, "bad"); moreBtn.disabled = false; } } }, "Older lines") : null;
    clear(main, top, list, h("div", { class: "row between lx-foot" }, h("span", { class: "small dim" }, `${fmt.n(rows.length)} lines, newest first${st.live && canTail() ? " · live" : ""}. Click a line for the lines around it.`), moreBtn));
  }

  /**
   * A line's parts: its time, level and host, then its text, and its key
   * and other fields (dimmer) when they are shown or the text is empty.
   */
  function lineParts(r, c) {
    const t = msOf(r[c.ts]), lv = c.sev ? r[c.sev] : null;
    const text = c.msg ? r[c.msg] : null, has = !blank(text);
    const showFields = fieldsShown() || !has;
    const kv = showFields ? rowFields(r, c) : [];
    const key = showFields && c.key && c.key !== c.msg && !blank(r[c.key]) ? h("span", { class: "lx-key" }, marked(shortValue(r[c.key]))) : null;
    // Spaces between the parts, so that a long line wraps between fields.
    const parts = [
      key,
      has ? h("span", { class: "lx-text" }, marked(c.log ? clip(String(text), 4000) : shortValue(text, "text", 300))) : null,
      kv.length ? h("span", { class: "lx-kvs" }, kv.flatMap(([k, v], i) => [i ? " " : null, h("span", { class: "lx-kv" }, h("span", { class: "lx-k" }, k === "…" ? "" : k + "="), marked(v))])) : null,
    ].filter(Boolean);
    return [
      h("span", { class: "lx-t", title: isFinite(t) ? new Date(t).toISOString() : "" }, isFinite(t) ? clock(t) : "–"),
      c.sev ? h("span", { class: "lx-lv " + tone(lv) }, lv ?? "") : null,
      c.host ? h("span", { class: "lx-host" }, r[c.host] ?? "") : null,
      h("span", { class: "lx-msg" }, parts.length ? parts.flatMap((p, i) => (i ? ["  ", p] : [p])) : h("span", { class: "lx-k" }, "(empty)")),
    ];
  }
  const clip = (x, n) => (x.length > n ? x.slice(0, n) + "…" : x);

  /** A line's text with the search's words marked. */
  function marked(text) {
    text = String(text ?? "");
    const terms = (st.terms || []).filter((t) => t && t.length > 0).map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/%/g, ".*?").replace(/_/g, "."));
    if (!terms.length) return text;
    const re = new RegExp(`(${terms.join("|")})`, "gi");
    const out = [];
    let last = 0, m;
    while ((m = re.exec(text)) && out.length < 60) {
      if (m[0] === "") { re.lastIndex++; continue; }
      if (m.index > last) out.push(text.slice(last, m.index));
      out.push(h("mark", {}, m[0]));
      last = m.index + m[0].length;
    }
    out.push(text.slice(last));
    return out;
  }

  function drawPatterns() {
    if (!st.patterns) return;
    const { groups, rows } = st.patterns;
    if (!groups.length) { clear(main, empty("search", "No lines", `No line of ${st.table} matches in ${timeRange().label}.`)); return; }
    const max = groups[0].n;
    const tbl = h("table", { class: "t compact lx-pats" },
      h("thead", {}, h("tr", {}, h("th", { class: "num" }, "lines"), h("th", {}, "pattern"), st.cols.host ? h("th", { class: "num" }, "hosts") : null, h("th", {}, "last seen"))),
      h("tbody", {}, groups.slice(0, 200).map((g) => h("tr", { class: "click", tabIndex: 0, title: g.sample, onclick: () => { st.pattern = g; st.tab = "lines"; writeUrl(); drawMain(); } },
        h("td", { class: "num" }, h("div", { class: "lx-bar-n" }, h("span", { style: { width: `${Math.max(2, (g.n / max) * 100)}%` } }), h("b", {}, fmt.n(g.n)))),
        h("td", { class: "lx-pat" }, g.pattern ? placeholders(g.pattern) : h("span", { class: "dim" }, "(no text)")),
        st.cols.host ? h("td", { class: "num", title: [...g.hosts].slice(0, 20).join(", ") }, fmt.n(g.hosts.size)) : null,
        h("td", { class: "small dim nw" }, isFinite(g.last) ? fmt.ago(g.last) : "–")))));
    clear(main, h("div", { class: "small dim", style: { padding: "8px 12px" } }, `${fmt.n(groups.length)} patterns in the newest ${fmt.n(rows)} matching lines${rows >= PATTERN_ROWS ? ` (the first ${fmt.n(PATTERN_ROWS)})` : ""}: numbers, addresses, ids and times are placeholders, grouped here in the page (SQL has no regular expressions to do it). Click one for its lines.`), h("div", { class: "tbl-wrap" }, tbl));
  }
  const placeholders = (p) => p.split(/(<(?:n|ip|mac|hex|uuid|time|str)>)/).map((x) => (/^<\w+>$/.test(x) ? h("span", { class: "lx-ph" }, x) : x));

  function drawHist() {
    const H = st.hist;
    if (!H) return;
    const r = timeRange(), b = H.bucket;
    const from = Math.floor(r.from / b.ms) * b.ms, to = r.to;
    const n = Math.max(1, Math.ceil((to - from) / b.ms));
    const bins = Array.from({ length: n }, (_, i) => ({ t: from + i * b.ms, bad: 0, warn: 0, rest: 0 }));
    let total = 0;
    for (const x of H.rows) {
      const i = Math.floor((x.t - from) / b.ms);
      if (i < 0 || i >= n) continue;
      const tn = tone(x.sev);
      bins[i][tn === "bad" ? "bad" : tn === "warn" ? "warn" : "rest"] += x.n;
      total += x.n;
    }
    const peak = Math.max(1, ...bins.map((x) => x.bad + x.warn + x.rest));
    const W = Math.max(320, histBox.clientWidth || 900), HT = 120, pad = 26, bw = (W - pad) / n;
    const svg = s("svg", { viewBox: `0 0 ${W} ${HT + 18}`, width: "100%", height: HT + 18, class: "lx-svg", role: "img", "aria-label": `Lines per ${b.label}` });
    svg.append(s("text", { x: 0, y: 10, class: "lx-ax" }, fmt.si(peak)), s("line", { x1: pad, x2: W, y1: HT, y2: HT, class: "lx-base" }));
    bins.forEach((x, i) => {
      const tot = x.bad + x.warn + x.rest;
      const g = s("g", { class: "lx-bin" });
      g.append(s("rect", { x: pad + i * bw, y: 0, width: Math.max(1, bw), height: HT, class: "lx-hit" }));
      let y = HT;
      for (const [k, cls] of [["rest", "lx-b-rest"], ["warn", "lx-b-warn"], ["bad", "lx-b-bad"]]) {
        if (!x[k]) continue;
        const hh = (x[k] / peak) * (HT - 14);
        y -= hh;
        g.append(s("rect", { x: pad + i * bw + (bw > 4 ? 0.5 : 0), y, width: Math.max(1, bw - (bw > 4 ? 1 : 0)), height: hh, class: cls }));
      }
      g.onmousemove = (e) => showTip(e.clientX, e.clientY, h("div", {}, h("b", {}, `${clock(x.t).slice(0, 5)} to ${clock(x.t + b.ms).slice(0, 5)}`), h("div", {}, `${fmt.n(tot)} lines`), x.bad ? h("div", { class: "lx-tip-bad" }, `${fmt.n(x.bad)} errors`) : null, x.warn ? h("div", {}, `${fmt.n(x.warn)} warnings`) : null, h("div", { class: "small dim" }, "click to zoom in")));
      g.onmouseleave = hideTip;
      g.onclick = () => { hideTip(); setWindow(x.t, x.t + b.ms); };
      svg.append(g);
    });
    // Time ticks: about six.
    const every_ = Math.max(1, Math.round(n / 6));
    for (let i = 0; i < n; i += every_) svg.append(s("text", { x: pad + i * bw, y: HT + 14, class: "lx-ax" }, clock(bins[i].t).slice(0, 5)));
    const sub = histCard.querySelector(".hd .sub");
    if (sub) sub.textContent = `${fmt.n(total)} lines in ${r.label}, per ${b.label}${st.cols.sev ? " · errors red, warnings amber" : ""} · click a bar to zoom in`;
    clear(histBox, svg);
  }

  function drawSql() {
    const order = ["Lines", "Lines over time", "Patterns (the lines)", "Older lines", "Live tail", "Lines around"];
    const items = [...st.ran.entries()].sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]));
    clear(sqlBox, items.map(([what, x]) => h("div", { class: "lx-ran" },
      h("div", { class: "row between" }, h("span", {}, h("b", {}, what), " ", h("span", { class: "small dim" }, x.error ? "failed: " + x.error : `${fmt.n(x.n)} rows · ${fmt.ms(x.ms)}`)),
        h("span", { class: "row nw", style: { gap: "6px" } }, h("a", { class: "btn xs ghost", href: link("sql", { q: x.sql }), title: "Open it in the SQL workbench" }, icon("sql"), "Workbench"), h("button", { class: "btn xs ghost", onclick: () => copy(x.sql, "SQL copied") }, icon("copy"), "Copy"))),
      sqlBlock(x.sql, { class: "code lx-code" }))));
  }

  // ---- the lines around one
  async function around(r) {
    const N = store.get("explorer.around", 20);
    const c = st.cols, tsv = r[c.ts];
    const same = c.host && r[c.host] != null ? [`${qn(c.host)} = ${lit(r[c.host])}`] : [];
    const box = h("div", { class: "stack" });
    const d = drawer({ title: "The lines around it", sub: `${same.length ? `${c.host} ${r[c.host]}, ` : ""}${new Date(msOf(tsv)).toISOString()}`, wide: true, body: box });
    async function draw(n) {
      store.set("explorer.around", n);
      const cond = (more) => [...same, more].join("\n  AND ");
      const before = `SELECT * FROM ${T()}\nWHERE ${cond(`${qn(c.ts)} <= ${timeLit(tsv)}`)}\nORDER BY ${qn(c.ts)} DESC\nLIMIT ${n + 1}`;
      const after = `SELECT * FROM ${T()}\nWHERE ${cond(`${qn(c.ts)} > ${timeLit(tsv)}`)}\nORDER BY ${qn(c.ts)} ASC\nLIMIT ${n}`;
      clear(box, h("div", { class: "dim small" }, "Reading…"));
      try {
        const [a, b] = await Promise.all([api("/v1/sql", { body: { sql: before } }), api("/v1/sql", { body: { sql: after } })]);
        st.ran.set("Lines around", { sql: `${before};\n\n${after};`, ms: (a.took_ms || 0) + (b.took_ms || 0), n: a.rows.length + b.rows.length });
        drawSql();
        const rows = [...objects(a).reverse(), ...objects(b)];
        const me = keyOf(r);
        const list = h("div", { class: "lx-lines" }, rows.map((x) => {
          const lv = c.sev ? x[c.sev] : null;
          return h("div", { class: `lx-line ${tone(lv)}${keyOf(x) === me ? " lx-me" : ""}` }, lineParts(x, c));
        }));
        const fields = h("table", { class: "t compact" }, h("tbody", {}, Object.entries(r).map(([k, v]) => h("tr", {}, h("td", { class: "dim nw" }, k), h("td", { class: "lx-msg" }, v == null ? "–" : typeof v === "object" ? JSON.stringify(v) : String(v))))));
        clear(box,
          h("div", { class: "row between" }, h("span", { class: "small dim" }, `${n} lines before and after it by time${same.length ? `, on ${r[c.host]}` : ""}`), seg([[10, "±10"], [20, "±20"], [50, "±50"], [200, "±200"]], n, draw)),
          list, h("h4", { style: { margin: "8px 0 0" } }, "This line"), fields,
          h("div", { class: "row between" }, h("b", {}, "The SQL it ran"), h("button", { class: "btn xs ghost", onclick: () => copy(`${before};\n\n${after};`, "SQL copied") }, icon("copy"), "Copy")), sqlBlock(`${before};\n\n${after};`, { class: "code lx-code" }));
        box.querySelector(".lx-me")?.scrollIntoView({ block: "center" });
      } catch (e) { clear(box, h("div", { class: "error" }, e.message)); }
    }
    draw(N);
    return d;
  }

  boot();
  return () => { alive = false; reparams = null; for (const f of stops) f(); for (const f of off) f(); hideTip(); };
}

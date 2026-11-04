// Guard rails for statements that change rows or tables, from the SQL
// workbench: what kind a statement is, which table it changes and with
// which WHERE; whether this tab may write (production is read-only until
// writes are allowed for 15 minutes, with a reason); and the check every
// write waits for: how many rows it changes (a SELECT COUNT(*) with the
// same WHERE), on how many shards, that it has no undo, and a reason the
// server keeps in its audit log. An UPDATE or DELETE without WHERE and a
// DROP need the table's name typed.

import { h, icon, clear, fmt, api, modal, sqlBlock, toast, ApiError, can } from "./core.js";
import { state, isProduction, environment, clusterName } from "./data.js";
import { table as tableOf } from "./catalog.js";
import { quoteName } from "./sqledit.js";

// ------------------------------------------------------------ reading a statement
/** The tokens of a statement, strings and comments skipped over, each with its depth in parentheses. */
export function tokens(sql) {
  const out = [];
  let i = 0, depth = 0;
  const n = sql.length;
  while (i < n) {
    const c = sql[i];
    if (/\s/.test(c)) { i++; continue; }
    if (c === "-" && sql[i + 1] === "-") { const e = sql.indexOf("\n", i); i = e < 0 ? n : e; continue; }
    if (c === "/" && sql[i + 1] === "*") { const e = sql.indexOf("*/", i + 2); i = e < 0 ? n : e + 2; continue; }
    if (c === "'" || c === '"' || c === "`") {
      let j = i + 1;
      for (; j < n; j++) { if (sql[j] === c) { if (sql[j + 1] === c) j++; else break; } }
      const raw = sql.slice(i, j + 1);
      out.push({ t: c === "'" ? "str" : "id", v: c === "'" ? raw : raw.slice(1, -1).split(c + c).join(c), raw, at: i, end: Math.min(n, j + 1), depth });
      i = j + 1; continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      const m = /^[A-Za-z_][\w$]*/.exec(sql.slice(i, i + 200));
      out.push({ t: "word", v: m[0], up: m[0].toUpperCase(), raw: m[0], at: i, end: i + m[0].length, depth });
      i += m[0].length; continue;
    }
    if (/[0-9]/.test(c) || (c === "." && /[0-9]/.test(sql[i + 1] || "")) || (c === "-" && /[0-9]/.test(sql[i + 1] || "") && !/^(num|str|id|word|param)$/.test(out[out.length - 1]?.t || "") && out[out.length - 1]?.v !== ")")) {
      const m = /^-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?/.exec(sql.slice(i, i + 60));
      out.push({ t: "num", v: m[0], raw: m[0], at: i, end: i + m[0].length, depth });
      i += m[0].length; continue;
    }
    if (c === ":" && /[A-Za-z_]/.test(sql[i + 1] || "") && sql[i - 1] !== ":") {
      const m = /^:[A-Za-z_]\w*/.exec(sql.slice(i, i + 100));
      out.push({ t: "param", v: m[0].slice(1), raw: m[0], at: i, end: i + m[0].length, depth });
      i += m[0].length; continue;
    }
    if (c === "(" || c === "[") { out.push({ t: "p", v: c, raw: c, at: i, end: i + 1, depth }); depth++; i++; continue; }
    if (c === ")" || c === "]") { depth = Math.max(0, depth - 1); out.push({ t: "p", v: c, raw: c, at: i, end: i + 1, depth }); i++; continue; }
    const two = sql.slice(i, i + 2);
    const op = ["<=", ">=", "<>", "!=", "||", "::", "=>"].includes(two) ? two : c;
    out.push({ t: "op", v: op, raw: op, at: i, end: i + op.length, depth });
    i += op.length;
  }
  return out;
}

const READS = new Set(["SELECT", "WITH", "VALUES", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "TABLE"]);
// Verbs that change something (and those Strata refuses, checked all the same).
const WRITES = new Set(["INSERT", "UPDATE", "DELETE", "REPLACE", "UPSERT", "MERGE", "TRUNCATE", "CREATE", "DROP", "ALTER", "COPY", "GRANT", "REVOKE"]);

/**
 * What a statement is: {kind, read, write, table, target, where, limit,
 * set: [fields], rows (INSERT VALUES), conflict ("fail"|"replace"|"ignore"),
 * keys (INSERT literal keys), drops: [tables], changes: [ALTER changes],
 * what (DROP/CREATE object)}. `kind` is the verb in capitals (UPDATE,
 * DELETE, INSERT, DROP, ALTER, CREATE, SELECT, ...), null for nothing.
 */
export function parseStatement(sql) {
  const ts = tokens(sql || "");
  let k = 0;
  while (ts[k]?.v === "(") k++;
  const first = ts[k];
  if (!first) return { kind: null, read: true, write: false };
  if (first.t !== "word") return { kind: first.raw, read: false, write: true };
  let kind = first.up;
  // WITH ... the main statement is the first verb outside the CTEs' parentheses.
  if (kind === "WITH") {
    const main = ts.slice(k + 1).find((t) => t.depth === 0 && t.t === "word" && ["SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE"].includes(t.up));
    if (main && main.up !== "SELECT") { kind = main.up; k = ts.indexOf(main); }
  }
  if (kind === "REPLACE") return insert(ts, k, sql, "replace");
  if (READS.has(kind)) return { kind: kind === "DESC" ? "DESCRIBE" : kind, read: true, write: false };
  // A word that is no statement: the server says so; nothing to check.
  if (!WRITES.has(kind)) return { kind, read: false, write: false, unknown: true };
  const base = { kind, read: false, write: true };
  const at0 = (i) => ts[i] && ts[i].depth === 0;
  // The text of a clause: from token a up to the next depth-0 word in `stops` (or the end).
  const until = (a, stops) => {
    let b = a;
    while (b < ts.length && !(at0(b) && ts[b].t === "word" && stops.has(ts[b].up))) b++;
    return { a, b, text: a < ts.length && b > a ? sql.slice(ts[a].at, ts[b - 1].end) : "" };
  };
  const limitOf = (i) => { const l = ts.findIndex((t, j) => j > i && t.depth === 0 && t.up === "LIMIT"); return l >= 0 && ts[l + 1]?.t === "num" ? Number(ts[l + 1].v) : null; };
  const nameAt = (i) => ts[i] && (ts[i].t === "word" || ts[i].t === "id") ? ts[i].v : null;

  if (kind === "UPDATE") {
    const tgt = until(k + 1, new Set(["SET"]));
    const s = ts.findIndex((t, j) => j > k && t.depth === 0 && t.up === "SET");
    const setPart = s >= 0 ? until(s + 1, new Set(["WHERE", "ORDER", "LIMIT", "RETURNING", "FROM"])) : { a: 0, b: 0, text: "" };
    const set = [];
    let expect = true;
    for (let i = setPart.a; i < setPart.b; i++) {
      if (expect && ts[i].depth === 0 && (ts[i].t === "word" || ts[i].t === "id")) { set.push(ts[i].v); expect = false; }
      if (ts[i].depth === 0 && ts[i].v === ",") expect = true;
    }
    const w = ts.findIndex((t, j) => j > k && t.depth === 0 && t.up === "WHERE");
    const where = w >= 0 ? until(w + 1, new Set(["ORDER", "LIMIT", "RETURNING"])).text : null;
    return { ...base, table: nameAt(k + 1), target: tgt.text, set, where, limit: limitOf(k) };
  }
  if (kind === "DELETE") {
    const f = ts.findIndex((t, j) => j > k && t.depth === 0 && t.up === "FROM");
    const tgt = f >= 0 ? until(f + 1, new Set(["WHERE", "ORDER", "LIMIT", "RETURNING", "USING"])) : { text: "" };
    const w = ts.findIndex((t, j) => j > k && t.depth === 0 && t.up === "WHERE");
    const where = w >= 0 ? until(w + 1, new Set(["ORDER", "LIMIT", "RETURNING"])).text : null;
    return { ...base, table: f >= 0 ? nameAt(f + 1) : null, target: tgt.text, where, limit: limitOf(k) };
  }
  if (kind === "INSERT") {
    let conflict = "fail";
    if (ts[k + 1]?.up === "OR") conflict = ts[k + 2]?.up === "REPLACE" ? "replace" : ts[k + 2]?.up === "IGNORE" ? "ignore" : "fail";
    return insert(ts, k, sql, conflict);
  }
  if (kind === "DROP" || kind === "CREATE") {
    let i = k + 1;
    const words = [];
    while (ts[i]?.t === "word" && ["TABLE", "RANK", "PROFILE", "IF", "EXISTS", "NOT", "OR", "REPLACE", "TEMPORARY", "TEMP", "INDEX", "VIEW"].includes(ts[i].up)) { words.push(ts[i].up); i++; }
    const what = words.includes("RANK") ? "rank profile" : words.includes("TABLE") ? "table" : words.includes("INDEX") ? "index" : words.includes("VIEW") ? "view" : (words[0] || "").toLowerCase();
    if (kind === "CREATE") return { ...base, what, table: nameAt(i) };
    const drops = [];
    for (; i < ts.length; i++) {
      if (ts[i].depth === 0 && (ts[i].t === "word" || ts[i].t === "id") && !["CASCADE", "RESTRICT", "ON"].includes(ts[i].up)) drops.push(ts[i].v);
      if (ts[i].up === "ON") { i++; continue; }
    }
    const onTable = ts.findIndex((t) => t.up === "ON");
    return { ...base, what, drops: what === "rank profile" ? drops.slice(0, 1) : drops, table: what === "rank profile" ? nameAt(onTable + 1) : drops[0] || null, cascade: ts.some((t) => t.up === "CASCADE") };
  }
  if (kind === "ALTER") {
    let i = k + 1;
    if (ts[i]?.up === "TABLE") i++;
    if (ts[i]?.up === "IF" && ts[i + 1]?.up === "EXISTS") i += 2;
    const table = nameAt(i);
    const changes = [];
    // Each change starts at ADD, DROP or RENAME at depth 0.
    const starts = [];
    for (let j = i + 1; j < ts.length; j++) if (ts[j].depth === 0 && ["ADD", "DROP", "RENAME"].includes(ts[j].up) && (j === i + 1 || ts[j - 1].v === ",")) starts.push(j);
    starts.forEach((s, x) => {
      const end = x + 1 < starts.length ? starts[x + 1] - 1 : ts.length;
      let j = s + 1;
      if (ts[j]?.up === "COLUMN") j++;
      if (ts[j]?.up === "IF") j += ts[j + 1]?.up === "NOT" ? 3 : 2;
      const col = nameAt(j);
      const op = ts[s].up;
      const to = op === "RENAME" ? nameAt(ts.findIndex((t, y) => y > j && t.up === "TO") + 1) : null;
      changes.push({ op, column: col, to, text: sql.slice(ts[s].at, ts[Math.min(end, ts.length) - 1].end) });
    });
    return { ...base, table, changes };
  }
  return { ...base, table: null };
}

function insert(ts, k, sql, conflict) {
  const into = ts.findIndex((t, j) => j > k && t.depth === 0 && t.up === "INTO");
  const nameTok = ts[into + 1];
  const table = nameTok && (nameTok.t === "word" || nameTok.t === "id") ? nameTok.v : null;
  // The columns, when listed.
  let cols = null, i = into + 2;
  if (ts[i]?.v === "(") {
    cols = [];
    for (i++; i < ts.length && !(ts[i].v === ")" && ts[i].depth === 0); i++) if (ts[i].depth === 1 && (ts[i].t === "word" || ts[i].t === "id")) cols.push(ts[i].v);
    i++;
  }
  const values = ts.findIndex((t, j) => j >= i && t.depth === 0 && t.up === "VALUES");
  const conflictAt = ts.findIndex((t, j) => j > k && t.depth === 0 && t.up === "ON" && ts[j + 1]?.up === "CONFLICT");
  if (conflictAt >= 0) {
    const rest = ts.slice(conflictAt).filter((t) => t.depth === 0).map((t) => t.up).join(" ");
    conflict = /DO UPDATE/.test(rest) ? "replace" : /DO NOTHING/.test(rest) ? "ignore" : conflict;
  }
  let rows = null, keys = null;
  const key = tableOf(table)?.short?.key;
  const ki = cols && key ? cols.findIndex((c) => c.toLowerCase() === key.toLowerCase()) : -1;
  if (values >= 0) {
    rows = 0; keys = ki >= 0 ? [] : null;
    const end = conflictAt >= 0 ? conflictAt : ts.length;
    for (let j = values + 1; j < end; j++) {
      if (ts[j].depth === 0 && ts[j].v === "(") {
        rows++;
        if (keys) {
          // The ki-th value of the tuple, when it is a literal.
          let pos = 0, lit = null;
          for (let y = j + 1; y < end && !(ts[y].depth === 0 && ts[y].v === ")"); y++) {
            if (ts[y].depth === 1 && ts[y].v === ",") { pos++; continue; }
            if (pos === ki && ts[y].depth === 1 && lit == null && (ts[y].t === "str" || ts[y].t === "num")) lit = ts[y].raw;
          }
          if (lit != null) keys.push(lit); else keys = null;
        }
      }
    }
  }
  return { kind: "INSERT", read: false, write: true, table, cols, rows, conflict, keys, fromSelect: values < 0 && ts.some((t, j) => j > into && t.depth === 0 && t.up === "SELECT") };
}

/** What the Run button says for a statement. */
export function runLabel(p, explain = false) {
  if (!p?.kind) return "Run";
  if (explain || p.kind === "EXPLAIN") return "Explain";
  if (p.write) return "Check " + p.kind;
  if (p.unknown) return "Run";
  if (p.kind === "WITH" || p.kind === "VALUES" || p.kind === "TABLE") return "Run SELECT";
  return "Run " + p.kind;
}

// ------------------------------------------------------------ writes allowed in this tab
// Production starts read-only; writes are allowed for 15 minutes with a
// reason, in this tab only (sessionStorage), for this server.
const ALLOW_MS = 15 * 60e3;
const allowKey = () => "strata.cc.sql.writes." + location.host;

/** Whether writes are allowed here: {until_ms, reason} while allowed, else null. */
export function writesAllowed() {
  try {
    const a = JSON.parse(sessionStorage.getItem(allowKey()) || "null");
    if (a && a.until_ms > Date.now()) return a;
  } catch { /* private mode: never allowed */ }
  return null;
}
export function allowWrites(reason) {
  const a = { until_ms: Date.now() + ALLOW_MS, reason, at_ms: Date.now() };
  try { sessionStorage.setItem(allowKey(), JSON.stringify(a)); } catch { /* private mode */ }
  // A note on the cluster's timeline, where the server keeps notes (admin keys).
  api("/v1/events/notes", { body: { text: `SQL workbench: writes allowed for 15 min. ${reason}`, where: {} }, reason }).catch(() => { /* an older server, or not an admin key */ });
  return a;
}
export function endWrites() { try { sessionStorage.removeItem(allowKey()); } catch { /* private mode */ } }

/** The workbench's mode: {readOnly, why, allowed, prod}. */
export function mode() {
  const prod = isProduction();
  if (!can("write")) return { readOnly: true, why: "key", prod };
  const allowed = writesAllowed();
  if (prod && !allowed) return { readOnly: true, why: "production", prod };
  return { readOnly: false, allowed, prod };
}

// ------------------------------------------------------------ the cluster a write reaches
/** Shards and copies, from the cluster state (or health): {shards, primaries, replicas, coordinator}. */
export function reach() {
  const sh = state.cluster?.shards || state.health?.shards;
  if (!sh?.length) return { shards: 1, replicas: 0, coordinator: false };
  const reps = sh.reduce((a, s) => a + Math.max(0, (s.replicas?.length || 1) - 1), 0);
  // Shards that take no writes now (no primary answers): a write fails there
  // and the shards that take writes still do their part.
  const noWrites = shardTrouble(sh).filter((s) => !s.writes);
  return { shards: sh.length, replicas: reps, coordinator: true, noWrites };
}
/**
 * The shards that serve no reads or take no writes now, from the cluster
 * state: {shard, why, writes, reads, words} ("serves no reads and takes no
 * writes").
 */
export function shardTrouble(sh = state.cluster?.shards || state.health?.shards || []) {
  return sh.map((s, i) => {
    const writes = !(s.takes_writes === false || s.state === "no_writes" || s.state === "down");
    const reads = !(s.serves_reads === false || s.state === "no_reads" || s.state === "down");
    return { shard: s.shard ?? i, why: s.why || "", writes, reads, words: [!reads && "serves no reads", !writes && "takes no writes"].filter(Boolean).join(" and ") };
  }).filter((s) => !s.writes || !s.reads);
}
/** "prod-eu1 · production", or the host when the cluster has no name. */
export function whereName() {
  const name = clusterName?.() || location.host;
  const env = environment?.();
  return { name, env };
}

// ------------------------------------------------------------ the dialogs
/** Asks to allow writes for 15 minutes; resolves {reason} or false. */
export function askAllow(p) {
  return new Promise((resolve) => {
    const { name, env } = whereName();
    const reason = h("input", { class: "input", placeholder: "INC-4821: archive the orders moved to cold storage", style: { width: "100%" }, maxlength: 300 });
    const ok = h("button", { class: "btn primary", disabled: true }, icon("unlock"), "Allow writes for 15 min");
    const no = h("button", { class: "btn" }, "Cancel");
    reason.oninput = () => { ok.disabled = reason.value.trim().length < 4; };
    let done = false;
    const m = modal({
      title: "Allow writes in this tab?",
      body: [
        h("p", { class: "dim", style: { margin: 0 } }, p?.kind ? `This is ${/^[AEIOU]/.test(p.kind) ? "an" : "a"} ${p.kind}. ` : "", `The workbench is read-only on ${name}${env ? ` (${env})` : ", whose environment is not set, so it counts as production"}. Allowing writes lasts 15 minutes, in this tab only. Every write still waits for its check.`),
        h("label", { class: "dt-lbl" }, "Why (kept with every write you run, and as a note on the timeline)", reason),
      ],
      foot: [no, ok],
      onClose: () => { if (!done) resolve(false); },
    });
    ok.onclick = () => { done = true; const r = reason.value.trim(); m.close(); allowWrites(r); resolve({ reason: r }); };
    no.onclick = () => { done = true; m.close(); resolve(false); };
    setTimeout(() => reason.focus(), 0);
  });
}

const qn = (n) => quoteName(n);
/** The rows a statement's count says, and the shards the plan reached. */
async function countRows(sql, params) {
  const t0 = performance.now();
  const j = await api("/v1/query", { body: { sql, params, timeout_ms: 15000 } });
  const routed = (j.stats?.plan || []).map((l) => /routed by key: (\d+) of (\d+) shards?/.exec(l)).find(Boolean);
  return { n: Number(j.rows?.[0]?.[0] ?? 0), ms: performance.now() - t0, at_ms: Date.now(), shards: routed ? Number(routed[1]) : null };
}
/** The size of one table, from GET /v1/index/tables when the server has it. */
async function tableSize(name) {
  try {
    const j = await api("/v1/index/tables");
    const list = Array.isArray(j) ? j : j.tables || [];
    return list.find((t) => t.table === name) || null;
  } catch { return null; }
}

function levenshtein(a, b) {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) for (let j = 1; j <= b.length; j++) d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
  return d[a.length][b.length];
}
/** Names near a word, closest first. */
export function nearNames(word, names) {
  const w = word.toLowerCase();
  return names.map((n) => [n, levenshtein(n.toLowerCase(), w)]).filter(([, d]) => d > 0 && d <= (w.length < 5 ? 1 : Math.max(2, Math.floor(w.length / 3)))).sort((a, b) => a[1] - b[1]).map(([n]) => n);
}

/**
 * The check a write waits for. `p` is parseStatement(sql). `onEdit(sql)`
 * puts a statement in the editor (never runs it); `fixesFor(message)`
 * gives [label, fixed sql] for an error the count met. Resolves {reason}
 * to run it, or false.
 */
export function checkWrite({ sql, params, p, onEdit, fixesFor }) {
  return new Promise((resolve) => {
    const { name: cluster, env } = whereName();
    const prod = isProduction();
    const r = reach();
    const t = tableOf(p.table);
    const noWhere = (p.kind === "UPDATE" || p.kind === "DELETE") && !p.where;
    const dropTable = p.kind === "DROP" && p.what === "table";
    const dropColumn = p.kind === "ALTER" && (p.changes || []).some((c) => c.op === "DROP");
    let typed = (noWhere && !p.limit) || dropTable ? (dropTable ? p.drops.join(", ") : p.table) : null;
    const allowed = writesAllowed();

    const rowsTile = h("div", { class: "dt-tile" });
    const shardsTile = h("div", { class: "dt-tile" });
    const fieldsTile = h("div", { class: "dt-tile" });
    const undoTile = h("div", { class: "dt-tile" });
    const problems = h("div", { class: "stack", style: { gap: "8px" } });
    const reason = h("input", { class: "input", style: { width: "100%" }, maxlength: 300, value: allowed?.reason || "", placeholder: prod ? "INC-4821: archive the orders moved to cold storage" : "optional here: what this write is for" });
    const typedField = () => h("input", { class: "input mono", style: { width: "100%" }, placeholder: "the table's name", autocomplete: "off", autocapitalize: "off", autocorrect: "off", spellcheck: "false", "aria-label": `Type ${typed} to run it` });
    let typedIn = typed ? typedField() : null;
    const typedBox = h("div", { class: "dt-typed" });
    const runBtn = h("button", { class: "btn " + (noWhere || dropTable || dropColumn ? "danger solid" : "primary"), disabled: true }, icon("play"),
      r.coordinator ? `Run on ${p.kind === "DROP" || p.kind === "ALTER" || p.kind === "CREATE" ? "every shard" : fmt.plural(r.shards, "shard")}` : "Run it");
    const cancel = h("button", { class: "btn" }, "Cancel");
    const addWhere = (p.kind === "UPDATE" || p.kind === "DELETE") && !p.where ? h("button", { class: "btn ghost", title: "Put a WHERE at the end of the statement, in the editor" }, icon("edit"), "Add a WHERE") : null;
    let counted = null, blocked = null, done = false;

    const tile = (el, label, value, sub, cls = "") => clear(el, h("div", { class: "small muted" }, label), h("div", { class: "dt-tv " + cls }, value), sub ? h("div", { class: "small dim" }, sub) : null);
    const shardsWord = r.coordinator ? `${fmt.n(r.shards)} of ${fmt.n(r.shards)}` : "this server";
    const shipped = r.replicas ? `, shipped to ${fmt.plural(r.replicas, "replica")}` : "";
    const nw = r.noWrites?.length || 0;

    // A shard without a primary: the coordinator refuses the whole write up front (coord.rs `writable`).
    if (r.noWrites?.length) {
      const names = r.noWrites.map((s) => `shard ${s.shard}`).join(", ");
      problems.append(h("div", { class: "banner bad dt-banner" }, icon("warn"), h("div", { class: "grow" },
        h("b", {}, r.noWrites.length > 1 ? `${names[0].toUpperCase() + names.slice(1)} take no writes now` : `Shard ${r.noWrites[0].shard} ${r.noWrites[0].words} now`),
        h("div", { class: "small" }, r.noWrites[0].why ? `${r.noWrites[0].why.replace(/^./, (c) => c.toUpperCase())}. ` : "",
          `If the statement writes rows of ${r.noWrites.length > 1 ? "them" : "it"}, the coordinator refuses all of it before any shard writes, so nothing is written. Wait until ${r.noWrites.length > 1 ? "they have" : "it has"} a primary, or write only keys of other shards. A shard that fails during the write is not undone on the others: the answer then lists each shard's part.`),
        h("div", { class: "small" }, h("a", { href: `#/cluster?shard=${r.noWrites[0].shard}`, title: "Cancels this write and opens the shard", onclick: () => cancel.click() }, "Open in Cluster")))));
    }
    // What each kind of statement changes.
    if (p.kind === "UPDATE" || p.kind === "DELETE") {
      tile(rowsTile, "Rows it changes", "counting…", `SELECT COUNT(*) with the same WHERE${p.limit ? `, at most ${fmt.n(p.limit)} (LIMIT)` : ""}`);
      tile(shardsTile, "Shards", shardsWord, r.coordinator ? (nw ? `${fmt.n(r.shards - nw)} of them take${r.shards - nw === 1 ? "s" : ""} writes now` : `written on every primary${shipped}`) : "one server", nw ? "warn" : "");
      if (p.kind === "UPDATE") {
        const fs = (p.set || []).map((n) => t?.short?.fields?.find((f) => f.name.toLowerCase() === n.toLowerCase()));
        tile(fieldsTile, "Fields set", (p.set || []).join(", ") || "–", fs.filter(Boolean).map((f) => `${f.type}${f.multi ? "[]" : ""}${f.indexed === false ? "" : " · indexed"}`).join("; ") || null);
        // A SET of a column the table does not have fails: say so before it runs.
        const fields = t?.short?.fields?.map((f) => f.name) || [];
        for (const n of t ? (p.set || []) : []) {
          if (fields.some((f) => f.toLowerCase() === n.toLowerCase())) continue;
          const near = nearNames(n, fields).slice(0, 3);
          blocked = `${n} is not a column of ${t.table}`;
          problems.append(h("div", { class: "banner bad dt-banner" }, icon("bad"), h("div", { class: "grow" },
            h("b", {}, `No column ${n} in ${t.table}`), h("div", { class: "small" }, `Its columns are ${fields.join(", ")}. The statement would fail on every shard.`),
            near.length ? h("div", { class: "row", style: { marginTop: "6px", gap: "6px" } }, near.map((x) => h("button", { class: "btn sm", title: `Write ${x} in place of ${n} in the editor; it does not run`, onclick: () => { finish(false); onEdit?.(replaceWord(sql, n, x)); } }, icon("edit"), `Write ${x} in its place`))) : null)));
        }
      } else tile(fieldsTile, "What goes", "whole rows", "every field of each row");
      tile(undoTile, "Undo", "none", "no transaction to roll back: write the rows back to undo it");
      if (noWhere) problems.prepend(h("div", { class: "banner bad dt-banner" }, icon("warn"), h("div", { class: "grow" },
        h("b", {}, `No WHERE: this ${p.kind === "UPDATE" ? "changes" : "deletes"} every row of ${p.table}${p.limit ? ` up to the LIMIT of ${fmt.n(p.limit)}` : ""}`),
        h("div", { class: "small" }, p.limit ? "Without WHERE, LIMIT picks the rows." : `${/^[AEIOU]/i.test(p.kind) ? "An" : "A"} ${p.kind} without WHERE runs only once you type the table's name. Add a WHERE to change fewer rows.`))));
      const countSql = `SELECT COUNT(*) FROM ${p.target || qn(p.table)}${p.where ? ` WHERE ${p.where}` : ""}`;
      // NEW-data-tools-6: the rows, not the text, ask for the typed name: every row of the table, or over 1,000.
      const allRows = p.where || p.limit != null ? countRows(`SELECT COUNT(*) FROM ${p.target || qn(p.table)}`, {}).then((c) => c.n).catch(() => null) : Promise.resolve(null);
      Promise.all([countRows(countSql, params), allRows]).then(([c, all]) => {
        counted = c;
        const n = p.limit != null ? Math.min(p.limit, c.n) : c.n;
        if (!typedIn && p.table && n > 0 && (n > 1000 || (all != null && n >= all))) {
          typed = p.table;
          typedIn = typedField();
          typedIn.oninput = sync;
          clear(typedBox, h("label", { class: "dt-lbl" }, h("span", {}, "Type ", h("b", { class: "mono" }, typed), ` to run it: it ${p.kind === "UPDATE" ? "changes" : "deletes"} ${all != null && n >= all ? "every row of the table" : `${fmt.n(n)} rows`}`), typedIn));
        }
        tile(rowsTile, "Rows it changes", fmt.n(n), `${p.where ? "" : "every row · "}counted with SELECT COUNT(*) in ${fmt.ms(c.ms)} at ${fmt.time(c.at_ms)}${p.limit != null && c.n > p.limit ? ` (${fmt.n(c.n)} match, LIMIT ${fmt.n(p.limit)})` : ""}`, noWhere || n > 1000 ? "bad" : "");
        rowsTile.title = countSql;
        if (c.shards != null && r.coordinator) tile(shardsTile, "Shards", `${fmt.n(c.shards)} of ${fmt.n(r.shards)}`, `the WHERE names keys: only the shards that hold them${shipped}`);
        if (n === 0) problems.append(h("div", { class: "banner dt-banner" }, icon("info"), h("div", { class: "grow" }, h("b", {}, "No row matches the WHERE"), h("div", { class: "small" }, "Running it changes nothing."))));
        sync();
      }).catch((e) => {
        const bad = e instanceof ApiError && e.status >= 400 && e.status < 500;
        tile(rowsTile, "Rows it changes", "unknown", bad ? "the count was refused" : `the count failed: ${e.message}`, "bad");
        if (bad) {
          blocked = e.message;
          const msg = String(e.message).replace(/^(query|sql) error: /i, "");
          const fixes = fixesFor?.(msg) || [];
          problems.append(h("div", { class: "banner bad dt-banner" }, icon("bad"), h("div", { class: "grow" }, h("b", {}, "The WHERE is refused"), h("div", { class: "small" }, msg, " The statement would fail the same way."),
            fixes.length ? h("div", { class: "row", style: { marginTop: "6px", gap: "6px" } }, fixes.map(([label, fixed]) => h("button", { class: "btn sm", title: "Writes the fix in the editor; it does not run", onclick: () => { finish(false); onEdit?.(fixed); } }, icon("edit"), label))) : null)));
        }
        sync();
      });
    } else if (p.kind === "INSERT") {
      tile(rowsTile, "Rows it writes", p.rows != null ? fmt.n(p.rows) : p.fromSelect ? "from a SELECT" : "–", p.fromSelect ? "as many as the SELECT returns" : null);
      tile(shardsTile, "Shards", r.coordinator ? (p.rows === 1 ? "1" : `up to ${fmt.n(r.shards)}`) : "this server", r.coordinator ? `each row to the shard of its key${shipped}` : null);
      tile(fieldsTile, "Fields", p.cols ? p.cols.join(", ") : "every field, in order", null);
      tile(undoTile, "Undo", "none", p.conflict === "replace" ? "a row it replaces is gone" : "delete the rows to undo it");
      if (p.keys?.length && p.table && t?.short?.key) {
        const keySql = `SELECT COUNT(*) FROM ${qn(p.table)} WHERE ${qn(t.short.key)} IN (${p.keys.slice(0, 1000).join(", ")})`;
        countRows(keySql, {}).then((c) => {
          if (!c.n) { tile(rowsTile, "Rows it writes", fmt.n(p.rows), "every key is new"); return; }
          const what = p.conflict === "replace" ? `${fmt.n(c.n)} replace rows that exist` : p.conflict === "ignore" ? `${fmt.n(c.n)} keys exist and are skipped` : `${fmt.n(c.n)} keys exist: the INSERT fails`;
          tile(rowsTile, "Rows it writes", fmt.n(p.rows), what, p.conflict === "fail" ? "bad" : "");
          sync();
        }).catch(() => {});
      }
    } else if (p.kind === "DROP") {
      tile(rowsTile, "Rows it deletes", p.what === "table" ? "counting…" : "none", p.what === "table" ? "every row of the table" : `drops the ${p.what}`);
      tile(shardsTile, "Shards", r.coordinator ? `every shard (${fmt.n(r.shards)})` : "this server", r.coordinator ? `and their ${fmt.plural(r.replicas, "replica")}` : null);
      tile(fieldsTile, "What goes", p.what === "table" ? (p.drops || []).join(", ") : (p.drops || [])[0] || "–", p.what === "table" ? "the table, its fields, its segments" : null);
      tile(undoTile, "Undo", "none", "restore it from a backup");
      if (dropTable) {
        Promise.all((p.drops || []).map((d) => countRows(`SELECT COUNT(*) FROM ${qn(d)}`, {}).then((c) => c.n).catch(() => null))).then(async (ns) => {
          const total = ns.reduce((a, n) => a + (n || 0), 0);
          const sizes = await Promise.all((p.drops || []).map(tableSize));
          const bytes = sizes.reduce((a, s) => a + (s?.bytes || 0), 0);
          tile(rowsTile, "Rows it deletes", ns.some((n) => n == null) ? "unknown" : fmt.n(total), bytes ? `${fmt.bytes(bytes)} on disk, one copy` : "every row of the table", "bad");
          sync();
        });
      }
      problems.append(h("div", { class: "banner bad dt-banner" }, icon("warn"), h("div", { class: "grow" }, h("b", {}, p.what === "table" ? `DROP TABLE deletes ${(p.drops || []).join(", ")} and every row in it` : `DROP removes the ${p.what}`), h("div", { class: "small" }, p.cascade ? "CASCADE also removes the references other tables hold to it." : "It cannot be undone."))));
    } else if (p.kind === "ALTER") {
      tile(rowsTile, "Rows", "kept", "ALTER writes only the schema");
      tile(shardsTile, "Shards", r.coordinator ? `every shard (${fmt.n(r.shards)})` : "this server", null);
      tile(fieldsTile, "Changes", (p.changes || []).map((c) => `${c.op} ${c.column || ""}${c.to ? " to " + c.to : ""}`).join(", ") || "–", null);
      tile(undoTile, "Undo", dropColumn ? "none" : "another ALTER", dropColumn ? "a dropped column's values are gone" : "rename back, or drop what was added");
      if (dropColumn && r.coordinator) problems.append(h("div", { class: "banner dt-banner" }, icon("info"), h("div", { class: "grow" }, h("b", {}, "A coordinator refuses DROP COLUMN"), h("div", { class: "small" }, "Its plans name columns by position. Drop the column on each shard's server, or keep it."))));
    } else {
      tile(rowsTile, "Rows", p.kind === "CREATE" ? "none yet" : "–", p.kind === "CREATE" ? `creates the ${p.what || "object"} ${p.table || ""}` : null);
      tile(shardsTile, "Shards", r.coordinator ? `every shard (${fmt.n(r.shards)})` : "this server", null);
      tile(fieldsTile, "What", p.kind, null);
      tile(undoTile, "Undo", p.kind === "CREATE" ? "DROP it" : "none", null);
    }

    function sync() {
      const needReason = prod && reason.value.trim().length < 4;
      const needTyped = typedIn && typedIn.value.trim() !== typed;
      const waiting = (p.kind === "UPDATE" || p.kind === "DELETE") && !counted && !rowsTile.querySelector(".bad");
      runBtn.disabled = !!blocked || needReason || needTyped || waiting;
      runBtn.title = blocked ? "The statement would fail: fix it first" : waiting ? "Counting the rows first" : needReason ? "Say why first" : needTyped ? `Type ${typed} first` : "";
    }
    reason.oninput = sync; if (typedIn) typedIn.oninput = sync;

    const m = modal({
      title: "Check this write before it runs",
      wide: true,
      body: [
        h("div", { class: "small dim" }, `${cluster}${env ? " · " + env : ""} · ${r.coordinator ? fmt.plural(r.shards, "shard") : "one server"} · ${state.whoami ? `${state.whoami.name} · ${state.whoami.scope} key` : "key unknown"}`),
        sqlBlock(sql.trim(), { style: { maxHeight: "22vh", flex: "none", overflow: "auto" } }),
        problems,
        h("div", { class: "dt-tiles" }, rowsTile, shardsTile, fieldsTile, undoTile),
        h("div", { class: "dt-fields" },
          h("label", { class: "dt-lbl" }, prod ? "Why (required: sent with the statement, kept in the audit log with the rows and your key)" : "Why (sent with the statement and kept in the audit log)", reason),
          typedIn ? h("label", { class: "dt-lbl" }, h("span", {}, "Type ", h("b", { class: "mono" }, typed), " to run it"), typedIn) : typedBox),
        h("div", { class: "small muted dt-hint" }, "Ctrl+Enter never skips this check. Writes from the command center are tagged so the server can tell them from client traffic."),
      ],
      foot: [cancel, addWhere, h("span", { class: "sep" }), runBtn],
      onClose: () => { if (!done) { done = true; resolve(false); } },
    });
    function finish(v) { if (done) return; done = true; m.close(); resolve(v); }
    cancel.onclick = () => finish(false);
    runBtn.onclick = () => { if (!runBtn.disabled) finish({ reason: reason.value.trim() }); };
    if (addWhere) addWhere.onclick = () => { finish(false); onEdit?.(sql.trimEnd().replace(/;?\s*$/, "") + `\nWHERE ${t?.short?.key ? qn(t.short.key) + " = " : ""}`); };
    sync();
    // Focus: the reason when it is needed, else Cancel; never the Run button.
    setTimeout(() => (prod && !reason.value ? reason : cancel).focus(), 0);
  });
}

/** `sql` with the word `from` (outside strings) written as `to`, once per place. */
export function replaceWord(sql, from, to) {
  const ts = tokens(sql).filter((t) => (t.t === "word" || t.t === "id") && t.v.toLowerCase() === from.toLowerCase());
  let out = sql;
  for (const t of ts.reverse()) out = out.slice(0, t.at) + quoteName(to) + out.slice(t.end);
  return out;
}

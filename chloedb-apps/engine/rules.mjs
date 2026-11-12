// Rules: functions and triggers written as data instead of code. Most
// functions are a query (a total, the rows due today), and most triggers keep
// a count or a stock level right or write a history row. A rule says that in
// SQL, so it needs no writer, no sandbox and no code check, and runs in the
// engine directly. Functions that need more stay JavaScript in QuickJS.
//
// A function's plan entry carries "rule" instead of a code block:
//   {"query": "SELECT SUM(amount) AS total FROM expenses WHERE day >= :from", "one": true}
//     a call returns the rows (one: the first row; value: its first column)
//   {"steps": [step, ...]}
//     {"update": "features", "id": ":row.feature", "set": {"votes": "SELECT COUNT(*) FROM votes WHERE feature = :row.feature"}}
//     {"insert": "moves", "values": {"item": ":row.id", "qty": ":qty", "reason": "manual"}}
//     {"remove": "holds", "id": ":row.hold"}
//     {"let": "stock", "value": "SELECT stock FROM items WHERE id = :item"}   (a name later steps use as :stock)
//     {"each": "SELECT product, qty FROM cart WHERE owner = :user.id", "as": "line", "do": [steps]}   (:line.qty inside)
//     {"require": "SELECT stock >= :line.qty FROM products WHERE id = :line.product", "else": "Only :stock left"}
//       (the rule stops, writes nothing, and the person sees the message)
//     {"upsert": "cart", "find": "SELECT id FROM cart WHERE product = :product", "values": {"qty": 1}}
//     {"return": ":order.id"}   (what app.call gives back; otherwise {done})
//     any step may have "if": a SELECT whose first value decides whether it runs
//   A rule with steps is one transaction: its SELECTs see the data as it was
//   when it started, its writes happen together when it ends (all or
//   nothing), then the triggers on them run.
//   Values: a SELECT (its first column of its first row), ":name" (the
//   input, or a let; ":row.col" reads into an object of the input, such as
//   the row a trigger fires on), or a literal. In SQL, :row.col is a
//   parameter like :name.

import { parseFeed, jsonItems, at as pathAt } from './feeds.mjs';

const SELECT_RE = /^\s*(select|with)\b/i;

// The input and lets as SQL parameters: {row: {feature: 'f1'}, qty: 2} ->
// {row_feature: 'f1', qty: 2}.
export function paramsOf(input, lets = {}) {
  const out = {};
  for (const src of [input || {}, lets]) {
    for (const [k, v] of Object.entries(src)) {
      if (v && typeof v === 'object' && !Array.isArray(v)) for (const [k2, v2] of Object.entries(v)) out[`${k}_${k2}`] = v2;
      else out[k] = v;
    }
  }
  return out;
}
// :row.feature -> :row_feature, outside quoted strings.
export const sqlOf = (sql) => String(sql).split(/('(?:[^']|'')*')/).map((part, i) => (i % 2 ? part : part.replace(/:([A-Za-z_][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)/g, ':$1_$2'))).join('');
// Only the parameters the query names are passed.
export function used(sql, params) {
  const out = {};
  for (const m of sql.matchAll(/:([A-Za-z_][A-Za-z0-9_]*)/g)) if (params[m[1]] !== undefined) out[m[1]] = params[m[1]];
  return out;
}

// A rule's refusal: the person sees the message; it is not an error to repair.
export class Refused extends Error {
  constructor(message) { super(message); this.refused = true; this.status = 409; }
}

const MAX_EACH = 1000;

export async function runRule(rule, input, host) {
  const lets = {};
  const query = async (sql) => { const q = sqlOf(sql); return host('sql', [q, used(q, paramsOf(input, lets))]); };
  const value = async (v) => {
    if (typeof v === 'string' && SELECT_RE.test(v)) { const rows = await query(v); const r = rows[0]; return r ? Object.values(r)[0] ?? null : null; }
    if (typeof v === 'string' && /^:[A-Za-z_][A-Za-z0-9_.]*$/.test(v)) { const p = paramsOf(input, lets); return p[v.slice(1).replace('.', '_')] ?? null; }
    if (v && typeof v === 'object' && !Array.isArray(v)) { const out = {}; for (const [k, x] of Object.entries(v)) out[k] = await value(x); return out; }
    return v;
  };
  if (rule.query) {
    const rows = await query(rule.query);
    if (rule.value) { const r = rows[0]; return r ? Object.values(r)[0] ?? null : null; }
    return rule.one ? rows[0] || null : rows;
  }
  const done = [];
  const fields = async (o) => { const out = {}; for (const [k, v] of Object.entries(o || {})) out[k] = await value(v); return out; };
  // ":name" in a message: the value it names.
  const say = (text) => String(text).replace(/:([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)?)/g, (m, n) => { const v = paramsOf(input, lets)[n.replace('.', '_')]; return v == null ? m : String(v); });
  // PULL: a feed's or a JSON list's items, each added once (by its key) to
  // a table. The fetch goes through app.fetch, so the owner's switches, the
  // hosts they allowed and the daily limits hold.
  const pulled = new Map(); // table -> keys added by this run
  const pull = async (p) => {
    const url = typeof p.url === 'string' && p.url.startsWith(':') ? await value(p.url) : p.url;
    if (!url) return { added: 0, items: 0 };
    // (fresh: a source wants the feed as it is now, not a copy kept 10 minutes)
    const page = await host('fetch', [String(url), { fresh: true, ...(p.secret ? { secret: p.secret } : {}) }]);
    if (page && page.status === 0) return { added: 0, items: 0 }; // a build's tests reach nothing
    if (!page || !page.ok) throw new Error(`PULL ${String(url).slice(0, 100)}: it answered ${page?.status ?? 'nothing'}`);
    let raw;
    try {
      raw = p.from === 'json'
        ? jsonItems(page.json !== undefined ? page.json : JSON.parse(page.text || ''), p.at || '')
        : parseFeed(page.body || page.text || '', page.url || String(url));
    } catch (e) { throw new Error(`PULL ${String(url).slice(0, 100)}: ${e.message}`); }
    const items = raw.slice(0, p.limit || 50).map((it) => Object.fromEntries(Object.entries(p.cols || {}).map(([c, f]) => [c, p.from === 'json' ? pathAt(it, f) : it[f]])));
    if (!pulled.has(p.into)) pulled.set(p.into, new Set());
    const seen = pulled.get(p.into);
    let added = 0;
    for (const row of items) {
      const k = row[p.key];
      if (k === undefined || k === null || k === '' || seen.has(String(k))) continue;
      seen.add(String(k));
      lets._pullkey = typeof k === 'object' ? JSON.stringify(k) : k;
      const have = await query(`SELECT id FROM ${p.into} WHERE ${p.key} = :_pullkey LIMIT 1`);
      delete lets._pullkey;
      if (have.length) continue;
      await host('insert', [p.into, row]);
      added++;
    }
    return { added, items: items.length };
  };
  const RETURN = Symbol('return');
  let result;
  const steps = async (list, path) => {
    for (const [i, s] of (list || []).entries()) {
      const where = `${path}[${i}]`;
      if (s.if !== undefined && !(await value(s.if))) continue;
      if (s.let) { lets[s.let] = await value(s.value); continue; }
      if (s.require !== undefined) {
        if (!(await value(s.require))) throw new Refused(say(s.else || 'This cannot be done right now.'));
        continue;
      }
      if (s.return !== undefined) { result = await value(s.return); return RETURN; }
      if (s.each !== undefined) {
        const rows = await query(s.each);
        if (rows.length > MAX_EACH) throw new Error(`${where}: "each" gave ${rows.length} rows; at most ${MAX_EACH}`);
        for (const row of rows) {
          lets[s.as || 'item'] = row;
          if ((await steps(s.do, `${where}.do`)) === RETURN) return RETURN;
        }
        continue;
      }
      if (s.update) {
        const id = await value(s.id);
        if (id == null) continue;
        const row = await host('update', [s.update, String(id), await fields(s.set)]);
        if (s.as) lets[s.as] = row;
        done.push(`updated ${s.update} ${id}`);
      } else if (s.upsert) {
        const id = await value(s.find);
        const vals = await fields(s.values);
        const row = id == null ? await host('insert', [s.upsert, vals]) : await host('update', [s.upsert, String(id), vals]);
        if (s.as) lets[s.as] = row;
        done.push(`${id == null ? 'added to' : 'updated'} ${s.upsert}${row && row.id ? ` ${row.id}` : ''}`);
      } else if (s.insert) {
        const row = await host('insert', [s.insert, await fields(s.values)]);
        if (s.as) lets[s.as] = row;
        done.push(`added to ${s.insert}${row && row.id ? ` ${row.id}` : ''}`);
      } else if (s.call) {
        const out = await host('call', [s.call, await fields(s.input)]);
        if (s.as) lets[s.as] = out;
        done.push(`called ${s.call}`);
      } else if (s.email) {
        // Sent once the rule's writes are stored (the engine holds it until then).
        const m = s.email;
        const to = typeof m.to === 'string' && SELECT_RE.test(m.to) ? (await query(m.to)).map((r) => Object.values(r)[0]) : [await value(m.to)];
        const list = [...new Set(to.flatMap((x) => String(x ?? '').split(',')).map((x) => x.trim()).filter(Boolean))];
        if (!list.length) throw new Refused('There is nobody to send the email to.');
        const bad = list.find((x) => !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(x));
        if (bad) throw new Refused(`${bad} is not an email address`);
        let text = m.text ? say(m.text) : '';
        if (m.rows) {
          const rows = await query(m.rows);
          const lines = rows.slice(0, MAX_EACH).map((r) => `- ${Object.values(r).filter((v) => v != null && v !== '').map((v) => (Array.isArray(v) ? v.join(', ') : String(v))).join(' · ')}`);
          text = [text, lines.length ? lines.join('\n') : say(m.none || '(none)')].filter(Boolean).join('\n\n');
        }
        await host('email', [{ to: list, subject: say(m.subject || ''), text }]);
        done.push(`emailed ${list.join(', ')}`);
      } else if (s.pull) {
        const got = await pull(s.pull);
        if (s.as) lets[s.as] = got;
        done.push(`pulled ${got.added} new into ${s.pull.into}`);
      } else if (s.remove) {
        const id = await value(s.id);
        if (id == null) continue;
        await host('remove', [s.remove, String(id)]);
        done.push(`removed ${s.remove} ${id}`);
      } else throw new Error(`${where}: a step is ${KINDS_TEXT}`);
    }
    return null;
  };
  if ((await steps(rule.steps, 'steps')) === RETURN) return result;
  // (names starting with "_" are the ones CREATE RULE made up)
  return { done, lets: Object.fromEntries(Object.entries(lets).filter(([k]) => !k.startsWith('_'))) };
}

const KINDS = ['update', 'insert', 'upsert', 'remove', 'call', 'email', 'pull', 'let', 'each', 'require', 'return'];
const KINDS_TEXT = 'update, insert, upsert, remove, call, email, pull, let, each, require or return';

// Checks a rule: its shape, that its SQL is a SELECT, and that the tables and
// columns it writes exist (columnsOf(table) -> Set | null).
export function checkRule(rule, columnsOf = () => null) {
  const problems = [];
  if (!rule || typeof rule !== 'object' || Array.isArray(rule)) return ['a rule is an object: {"query": ...} or {"steps": [...]}'];
  const sql = (v, where) => {
    if (typeof v === 'string' && /^\s*(insert|update|delete|create|drop|alter)\b/i.test(v)) problems.push(`${where}: only SELECT here; write with update, insert and remove steps`);
    if (typeof v === 'string' && v.startsWith('=')) problems.push(`${where}: a value here is a SELECT, ":name" or a literal; not an =expression`);
  };
  if (rule.query !== undefined) {
    if (typeof rule.query !== 'string' || !SELECT_RE.test(rule.query)) problems.push('query: give a SELECT');
    return problems;
  }
  if (!Array.isArray(rule.steps) || !rule.steps.length) return [`give "query" (a SELECT) or "steps" (a list of ${KINDS_TEXT} steps)`];
  const check = (list, path) => list.forEach((s, i) => {
    const where = `${path}[${i}]`;
    if (!s || typeof s !== 'object') { problems.push(`${where}: a step is an object`); return; }
    if (s.if !== undefined) { sql(s.if, `${where}.if`); if (typeof s.if !== 'string' || !SELECT_RE.test(s.if)) problems.push(`${where}.if: give a SELECT whose first value says whether the step runs`); }
    const kind = KINDS.find((k) => s[k] !== undefined);
    if (!kind) { problems.push(`${where}: a step is ${KINDS_TEXT}`); return; }
    if (kind === 'let') { sql(s.value, `${where}.value`); return; }
    if (kind === 'return') { if (typeof s.return === 'string') sql(s.return, `${where}.return`); return; }
    if (kind === 'require') {
      if (typeof s.require !== 'string' || !SELECT_RE.test(s.require)) problems.push(`${where}.require: give a SELECT whose first value must be true (or not 0) for the rule to go on`);
      if (s.else !== undefined && typeof s.else !== 'string') problems.push(`${where}.else: the message the person sees, as text`);
      return;
    }
    if (kind === 'each') {
      if (typeof s.each !== 'string' || !SELECT_RE.test(s.each)) problems.push(`${where}.each: give a SELECT; its rows are gone through one by one`);
      if (!Array.isArray(s.do) || !s.do.length) problems.push(`${where}: give "do": the steps for each row (the row is :${s.as || 'item'}.col)`);
      else check(s.do, `${where}.do`);
      return;
    }
    if (kind === 'call') { for (const [k, v] of Object.entries(s.input || {})) sql(v, `${where}.input.${k}`); return; }
    if (kind === 'email') {
      const m = s.email;
      if (!m || typeof m !== 'object') { problems.push(`${where}.email: {"to", "subject", "text", "rows"}`); return; }
      if (m.to === undefined || m.to === null || m.to === '') problems.push(`${where}.email.to: who it goes to (:to, :user.email, an address, or a SELECT of addresses)`);
      sql(m.to, `${where}.email.to`);
      if (!m.subject && !m.text) problems.push(`${where}.email: give it a subject or a text`);
      if (m.rows !== undefined && (typeof m.rows !== 'string' || !SELECT_RE.test(m.rows))) problems.push(`${where}.email.rows: give a SELECT, one line of the email per row`);
      return;
    }
    if (kind === 'pull') {
      const p = s.pull;
      if (!p || typeof p !== 'object') { problems.push(`${where}.pull: {"from": "feed" | "json", "url", "into", "cols", "key"}`); return; }
      if (!['feed', 'json'].includes(p.from)) problems.push(`${where}.pull.from: "feed" or "json"`);
      if (typeof p.url !== 'string' || !(/^https?:\/\//i.test(p.url) || /^:[A-Za-z_][A-Za-z0-9_.]*$/.test(p.url))) problems.push(`${where}.pull.url: an http or https address, or :name`);
      const cols = columnsOf(p.into);
      if (!cols) { problems.push(`${where}.pull: no table ${JSON.stringify(p.into)} in the app`); return; }
      if (!p.cols || typeof p.cols !== 'object' || !Object.keys(p.cols).length) problems.push(`${where}.pull.cols: {column: field}`);
      for (const c of Object.keys(p.cols || {})) if (!cols.has(c)) problems.push(`${where}.pull: ${c} is not a column of ${p.into}`);
      if (!p.key || !(p.key in (p.cols || {}))) problems.push(`${where}.pull.key: the column among cols that tells items apart`);
      return;
    }
    const cols = columnsOf(s[kind]);
    if (!cols) { problems.push(`${where}: no table ${JSON.stringify(s[kind])} in the app`); return; }
    if ((kind === 'update' || kind === 'remove') && s.id === undefined) problems.push(`${where}: give "id", the row to ${kind} (such as ":row.feature")`);
    if (kind === 'upsert' && (typeof s.find !== 'string' || !SELECT_RE.test(s.find))) problems.push(`${where}.find: give a SELECT of the id of the row to update (when there is none, a row is added)`);
    sql(s.id, `${where}.id`);
    const vals = kind === 'update' ? s.set : kind === 'insert' || kind === 'upsert' ? s.values : null;
    if (kind !== 'remove' && (!vals || typeof vals !== 'object')) problems.push(`${where}: give "${kind === 'update' ? 'set' : 'values'}": {column: value}`);
    for (const [k, v] of Object.entries(vals || {})) {
      if (!cols.has(k)) problems.push(`${where}: ${k} is not a column of ${s[kind]}`);
      sql(v, `${where}.${k}`);
    }
  });
  check(rule.steps, 'steps');
  return problems;
}

export const RULES_DOC = `# Rules: functions and triggers as data

A function that is a query, or does a few writes (a checkout, an invoice from hours, a purchase order, a stock move), or a trigger that keeps a count, a total or a stock level right, is a rule: give "rule" in its plan entry and no code block (it has no writer). Write JavaScript only for what a rule cannot say (text or date logic beyond SQL, the model, outside services): email is an "email" step.
- A query: {"query": "SELECT SUM(amount) AS total FROM expenses WHERE day >= :from"} returns the rows; add "one": true for the first row, or "value": true for its first column. app.call('slug', {from: '2026-11-01'}) passes :from. Similar rows and "bought together" are queries too (similarity, ppr).
- Steps, run in order: {"steps": [
    {"require": "SELECT stock >= :qty FROM products WHERE id = :product", "else": "Only :stock left"}  (when its first value is false or 0, the rule stops, writes nothing, and the person sees the message; :name in it is filled in)
    {"insert": "orders", "values": {"customer": ":user.id", "status": "new"}, "as": "order"},
    {"each": "SELECT c.product, c.qty, p.price FROM cart c JOIN products p ON p.id = c.product WHERE c.owner = :user.id", "as": "line", "do": [
      {"require": "SELECT stock >= :line.qty FROM products WHERE id = :line.product", "else": "Not enough in stock"},
      {"insert": "order_lines", "values": {"order": ":order.id", "product": ":line.product", "qty": ":line.qty", "price": ":line.price"}},
      {"update": "products", "id": ":line.product", "set": {"stock": "SELECT stock - :line.qty FROM products WHERE id = :line.product"}}]},
    {"upsert": "cart", "find": "SELECT id FROM cart WHERE owner = :user.id AND product = :product", "values": {"owner": ":user.id", "product": ":product", "qty": ":qty"}}  (updates the row found, or adds one),
    {"remove": "holds", "id": ":row.hold"},
    {"let": "left", "value": "SELECT stock FROM items WHERE id = :item"}, then :left in later steps,
    {"call": "reorder", "input": {"item": ":row.id"}}  (another function of the app),
    {"email": {"to": ":to", "subject": "New requests", "text": "Hello :user.name", "rows": "SELECT title, votes FROM requests ORDER BY votes DESC", "none": "Nothing new"}}  (sent once the writes are stored; a line per row; "to" may be a SELECT of addresses),
    {"return": ":order.id"}  (what app.call gives back)
  ]}; any step may have "if": a SELECT whose first value decides whether it runs, and an insert, update, upsert or call may have "as": a name for its result in later steps (:order.id).
- A rule with steps is one transaction: its SELECTs see the data as it was when it started (not its own writes: compute a total from the cart, not from the order lines it adds), its writes happen together at the end or not at all, and two runs never overlap, so "require" really holds (nothing is sold twice). Triggers on its writes run after.
- Values: a SELECT (its first value), ":name" (the call's input, or a let), ":row.col" (a column of the row a trigger fires on: a trigger gets {row, table, event}), ":user.id" (the signed-in person, also :user.name and :user.email; null when nobody is), or a literal. The same names work as parameters inside the SQL. Recompute a count or total with a SELECT rather than adding one, so deletes and edits stay right (a trigger on insert and another on delete can share the same steps).
- A rule is checked when the version is built: its SQL is SELECT only, and the tables and columns it writes must exist.`;

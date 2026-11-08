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
//     any step may have "if": a SELECT whose first value decides whether it runs
//   Values: a SELECT (its first column of its first row), ":name" (the
//   input, or a let; ":row.col" reads into an object of the input, such as
//   the row a trigger fires on), or a literal. In SQL, :row.col is a
//   parameter like :name.

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
  for (const [i, s] of (rule.steps || []).entries()) {
    if (s.if !== undefined && !(await value(s.if))) continue;
    if (s.let) { lets[s.let] = await value(s.value); continue; }
    const fields = async (o) => { const out = {}; for (const [k, v] of Object.entries(o || {})) out[k] = await value(v); return out; };
    if (s.update) {
      const id = await value(s.id);
      if (id == null) continue;
      await host('update', [s.update, String(id), await fields(s.set)]);
      done.push(`updated ${s.update} ${id}`);
    } else if (s.insert) {
      const row = await host('insert', [s.insert, await fields(s.values)]);
      if (s.as) lets[s.as] = row;
      done.push(`added to ${s.insert}${row && row.id ? ` ${row.id}` : ''}`);
    } else if (s.call) {
      const out = await host('call', [s.call, await fields(s.input)]);
      if (s.as) lets[s.as] = out;
      done.push(`called ${s.call}`);
    } else if (s.remove) {
      const id = await value(s.id);
      if (id == null) continue;
      await host('remove', [s.remove, String(id)]);
      done.push(`removed ${s.remove} ${id}`);
    } else throw new Error(`step ${i}: a step is update, insert, remove, call or let`);
  }
  return { done, lets };
}

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
  if (!Array.isArray(rule.steps) || !rule.steps.length) return ['give "query" (a SELECT) or "steps" (a list of update, insert, remove and let steps)'];
  rule.steps.forEach((s, i) => {
    const where = `steps[${i}]`;
    if (!s || typeof s !== 'object') { problems.push(`${where}: a step is an object`); return; }
    if (s.if !== undefined) { sql(s.if, `${where}.if`); if (typeof s.if !== 'string' || !SELECT_RE.test(s.if)) problems.push(`${where}.if: give a SELECT whose first value says whether the step runs`); }
    const kind = ['update', 'insert', 'remove', 'call', 'let'].find((k) => s[k] !== undefined);
    if (!kind) { problems.push(`${where}: a step is update, insert, remove, call or let`); return; }
    if (kind === 'let') { sql(s.value, `${where}.value`); return; }
    if (kind === 'call') { for (const [k, v] of Object.entries(s.input || {})) sql(v, `${where}.input.${k}`); return; }
    const cols = columnsOf(s[kind]);
    if (!cols) { problems.push(`${where}: no table ${JSON.stringify(s[kind])} in the app`); return; }
    sql(s.id, `${where}.id`);
    if ((kind === 'update' || kind === 'remove') && s.id === undefined) problems.push(`${where}: give "id", the row to ${kind} (such as ":row.feature")`);
    const vals = kind === 'update' ? s.set : kind === 'insert' ? s.values : null;
    if (kind !== 'remove' && (!vals || typeof vals !== 'object')) problems.push(`${where}: give "${kind === 'update' ? 'set' : 'values'}": {column: value}`);
    for (const [k, v] of Object.entries(vals || {})) {
      if (!cols.has(k)) problems.push(`${where}: ${k} is not a column of ${s[kind]}`);
      sql(v, `${where}.${k}`);
    }
  });
  return problems;
}

export const RULES_DOC = `# Rules: functions and triggers as data

A function that is a query, or a trigger that keeps a count, a total or a stock level right or writes a history row, is a rule: give "rule" in its plan entry and no code block (it has no writer). Write JavaScript only for what a rule cannot say.
- A query: {"query": "SELECT SUM(amount) AS total FROM expenses WHERE day >= :from"} returns the rows; add "one": true for the first row, or "value": true for its first column. app.call('slug', {from: '2026-11-01'}) passes :from.
- Steps, run in order: {"steps": [
    {"update": "features", "id": ":row.feature", "set": {"votes": "SELECT COUNT(*) FROM votes WHERE feature = :row.feature"}},
    {"insert": "stock_moves", "values": {"item": ":row.id", "qty": ":qty", "reason": "restock"}},
    {"remove": "holds", "id": ":row.hold"},
    {"let": "left", "value": "SELECT stock FROM items WHERE id = :item"}, then :left in later steps,
    {"call": "reorder", "input": {"item": ":row.id"}}  (another function of the app)
  ]}; any step may have "if": a SELECT whose first value decides whether it runs, and an insert or call may have "as": a name for its result in later steps (:name.id).
- Values: a SELECT (its first value), ":name" (the call's input, or a let), ":row.col" (a column of the row a trigger fires on: a trigger gets {row, table, event}), ":user.id" (the signed-in person, also :user.name and :user.email; null when nobody is), or a literal. The same names work as parameters inside the SQL. Recompute a count or total with a SELECT rather than adding one, so deletes and edits stay right (a trigger on insert and another on delete can share the same steps).
- A rule is checked when the version is built: its SQL is SELECT only, and the tables and columns it writes must exist.`;

// CREATE RULE: rules written as SQL. A rule (engine/rules.mjs) is a query, or
// steps the engine runs as one transaction; written as JSON it is long and
// unlike anything a model or a person has seen. Written as SQL it reads like
// a stored procedure:
//
//   CREATE RULE checkout AS
//     REQUIRE (SELECT COUNT(*) FROM cart WHERE owner = :user.id) > 0 ELSE 'Your cart is empty';
//     INSERT INTO orders (customer, status) VALUES (:user.id, 'new') AS order;
//     FOR line IN (SELECT c.id, c.product, c.qty, p.price, p.stock, p.name FROM cart c JOIN products p ON p.id = c.product WHERE c.owner = :user.id) DO
//       REQUIRE :line.stock >= :line.qty ELSE 'Only :line.stock left of :line.name';
//       INSERT INTO order_lines (order_id, product, qty, price) VALUES (:order.id, :line.product, :line.qty, :line.price);
//       UPDATE products SET stock = stock - :line.qty WHERE id = :line.product;
//       DELETE FROM cart WHERE id = :line.id;
//     END;
//     RETURN :order.id;
//
//   CREATE RULE count_votes AFTER INSERT OR DELETE ON votes AS
//     UPDATE features SET votes = (SELECT COUNT(*) FROM votes WHERE feature = :row.feature) WHERE id = :row.feature;
//
//   CREATE RULE spent RETURNS VALUE AS SELECT SUM(amount) FROM expenses WHERE day >= :from;
//
// compileRule() turns a statement into those steps, so the engine, its checks
// and its transaction are the same; the statement is kept as the rule's
// source, shown to the model and to people instead of the JSON.

export class RuleSqlError extends Error {}
const fail = (msg) => { throw new RuleSqlError(msg); };

const SELECT_RE = /^\s*(select|with)\b/i;
const NAME = '[A-Za-z_][A-Za-z0-9_]*';
const PARAM_RE = new RegExp(`^:${NAME}(?:\\.${NAME})?$`);
const EVENTS = ['insert', 'update', 'delete'];

// Drops -- comments, outside quotes.
function uncomment(text) {
  let out = '';
  for (let i = 0, q = null; i < text.length; i++) {
    const ch = text[i];
    if (q) { out += ch; if (ch === q) q = null; continue; }
    if (ch === "'" || ch === '"') { q = ch; out += ch; continue; }
    if (ch === '-' && text[i + 1] === '-') { while (i < text.length && text[i] !== '\n') i++; out += '\n'; continue; }
    out += ch;
  }
  return out;
}

// Walks text at the top level: fn(i, depth) for each character outside
// quotes; stops when fn returns true. Returns the index it stopped at, or -1.
function scan(text, fn) {
  let depth = 0;
  for (let i = 0, q = null; i < text.length; i++) {
    const ch = text[i];
    if (q) { if (ch === q) q = null; continue; }
    if (ch === "'" || ch === '"') { q = ch; continue; }
    if (ch === '(') depth++;
    else if (ch === ')') depth--;
    else if (fn(i, depth)) return i;
  }
  return -1;
}

// Splits at a character found at the top level (";" or ",").
function splitTop(text, sep) {
  const parts = [];
  let from = 0;
  scan(text, (i, d) => { if (d === 0 && text[i] === sep) { parts.push(text.slice(from, i)); from = i + 1; } return false; });
  parts.push(text.slice(from));
  return parts;
}

// The index of the first top-level keyword (a word, matched whole), or -1.
function findWord(text, word, from = 0) {
  const re = new RegExp(`^${word}\\b`, 'i');
  return scan(text, (i, d) => d === 0 && i >= from && (i === 0 || /[\s)]/.test(text[i - 1])) && re.test(text.slice(i, i + word.length + 1)));
}

// "(...)" at the start of text: [inside, rest], or null.
function group(text) {
  const t = text.trimStart();
  if (!t.startsWith('(')) return null;
  let depth = 0;
  for (let i = 0, q = null; i < t.length; i++) {
    const ch = t[i];
    if (q) { if (ch === q) q = null; continue; }
    if (ch === "'" || ch === '"') { q = ch; continue; }
    if (ch === '(') depth++;
    else if (ch === ')' && --depth === 0) return [t.slice(1, i), t.slice(i + 1)];
  }
  return fail(`a "(" has no ")": ${t.slice(0, 40)}`);
}

const unquote = (s) => s.slice(1, -1).replace(/''/g, "'");
const isWholeGroup = (e) => { const g = e.startsWith('(') && group(e); return !!g && !g[1].trim(); };

// A value as a rule step holds it: ":name", a literal, or a SELECT whose first
// value it is.
export function valueOf(expr) {
  const e = String(expr).trim();
  if (!e) fail('a value is missing');
  if (PARAM_RE.test(e)) return e;
  if (/^'(?:[^']|'')*'$/.test(e)) { const s = unquote(e); return SELECT_RE.test(s) || s.startsWith(':') ? `SELECT ${e}` : s; }
  if (/^-?\d+(\.\d+)?$/.test(e)) return Number(e);
  if (/^null$/i.test(e)) return null;
  if (/^(true|false)$/i.test(e)) return /^true$/i.test(e);
  if (isWholeGroup(e)) { const inner = group(e)[0].trim(); return SELECT_RE.test(inner) ? inner : valueOf(inner); }
  if (SELECT_RE.test(e)) return e;
  return `SELECT ${e}`;
}

// A value as SQL text, to put inside another query.
const sqlOf = (expr) => { const e = String(expr).trim(); return PARAM_RE.test(e) || /^'(?:[^']|'')*'$/.test(e) || /^-?\d+(\.\d+)?$/.test(e) || /^(null|true|false)$/i.test(e) || isWholeGroup(e) ? e : `(${e})`; };
// A condition as a SELECT whose first value decides.
const condOf = (expr) => { const e = String(expr).trim(); if (SELECT_RE.test(e)) return e; if (isWholeGroup(e)) { const inner = group(e)[0].trim(); if (SELECT_RE.test(inner)) return inner; } return `SELECT ${e}`; };
// A step that runs only when a condition (a SELECT) holds, and its own.
const only = (s, cond) => ({ ...s, if: s.if ? `SELECT (${cond}) AND (${s.if})` : cond });

let seq = 0;
const tmp = (p) => `_${p}${++seq}`;

// UPDATE t SET a = e, ... : each value; an expression over the row's own
// columns (stock - :qty) is read from the row.
function setOf(assigns, table, idSql) {
  const set = {};
  for (const a of splitTop(assigns, ',')) {
    const m = a.match(new RegExp(`^\\s*(${NAME})\\s*=\\s*([\\s\\S]+)$`));
    if (!m) fail(`SET ${a.trim().slice(0, 40)}: write column = value`);
    const e = m[2].trim();
    const v = valueOf(e);
    set[m[1]] = typeof v === 'string' && v === `SELECT ${e}` ? `SELECT ${e} FROM ${table} WHERE id = ${idSql}` : v;
  }
  return set;
}

// The statements of a rule's body, as steps.
function parseList(q, closers) {
  const steps = [];
  while (q.length) {
    const c = q[0].trim();
    if (!c) { q.shift(); continue; }
    const end = c.match(/^END(?:\s+(?:FOR|IF|LOOP))?\b/i);
    if (end) {
      if (!closers.includes('END')) fail('END without FOR or IF');
      q[0] = c.slice(end[0].length);
      return { steps, closer: 'END' };
    }
    if (/^ELSE\b/i.test(c) && closers.includes('ELSE')) { q[0] = c.slice(4); return { steps, closer: 'ELSE' }; }
    q.shift();
    steps.push(...parseStatement(c, q));
  }
  if (closers.length) fail('a FOR or IF has no END');
  return { steps, closer: null };
}

function parseStatement(c, q) {
  let m;
  // FOR name IN (SELECT ...) DO ... END
  if ((m = c.match(new RegExp(`^FOR\\s+(?:EACH\\s+)?(${NAME})\\s+IN\\b`, 'i')))) {
    let rest = c.slice(m[0].length).trim();
    let sql;
    if (rest.startsWith('(')) { const g = group(rest); sql = g[0].trim(); rest = g[1].trim(); }
    else { const at = findWord(rest, 'DO'); if (at < 0) fail(`FOR ${m[1]}: write FOR ${m[1]} IN (SELECT ...) DO ... END`); sql = rest.slice(0, at).trim(); rest = rest.slice(at); }
    if (!/^DO\b/i.test(rest)) fail(`FOR ${m[1]} IN (...): DO is missing`);
    if (!SELECT_RE.test(sql)) fail(`FOR ${m[1]} IN: give a SELECT; its rows are gone through one by one`);
    q.unshift(rest.slice(2));
    const body = parseList(q, ['END']);
    if (!body.steps.length) fail(`FOR ${m[1]}: no statements between DO and END`);
    return [{ each: sql, as: m[1], do: body.steps }];
  }
  // IF cond THEN ... [ELSE ...] END
  if (/^IF\b/i.test(c)) {
    const at = findWord(c, 'THEN');
    if (at < 0) fail('IF: write IF condition THEN ... END');
    const cond = c.slice(2, at).trim();
    q.unshift(c.slice(at + 4));
    const yes = parseList(q, ['END', 'ELSE']);
    const no = yes.closer === 'ELSE' ? parseList(q, ['END']).steps : [];
    const when = condOf(cond);
    const unless = `SELECT NOT ((${when}))`;
    return [...yes.steps.map((s) => only(s, when)), ...no.map((s) => only(s, unless))];
  }
  // REQUIRE cond [ELSE 'message']
  if (/^REQUIRE\b/i.test(c)) {
    const body = c.slice(7);
    const at = findWord(body, 'ELSE');
    const cond = (at < 0 ? body : body.slice(0, at)).trim();
    if (!cond) fail('REQUIRE: give the condition that must hold');
    const step = { require: condOf(cond) };
    if (at >= 0) {
      const msg = body.slice(at + 4).trim();
      if (!/^'(?:[^']|'')*'$/.test(msg)) fail("REQUIRE ... ELSE: the message is text in single quotes ('Out of stock')");
      step.else = unquote(msg);
    }
    return [step];
  }
  // LET name = value  (or = CALL fn(...))
  if ((m = c.match(new RegExp(`^LET\\s+(${NAME})\\s*(?::=|=)\\s*([\\s\\S]+)$`, 'i')))) {
    if (/^CALL\b/i.test(m[2].trim())) return parseStatement(`${m[2].trim()} AS ${m[1]}`, q);
    return [{ let: m[1], value: valueOf(m[2]) }];
  }
  if ((m = c.match(/^RETURN\s+([\s\S]+)$/i))) return [{ return: valueOf(m[1]) }];
  // CALL fn(name => value, ...) [AS name]
  if ((m = c.match(/^CALL\s+([a-z][a-z0-9_-]*)\s*/i))) {
    let rest = c.slice(m[0].length);
    const input = {};
    if (rest.trim().startsWith('(')) {
      const g = group(rest);
      rest = g[1];
      for (const a of splitTop(g[0], ',').filter((x) => x.trim())) {
        const am = a.match(new RegExp(`^\\s*(${NAME})\\s*(?:=>|:=|=)\\s*([\\s\\S]+)$`));
        if (!am) fail(`CALL ${m[1]}: name each input (CALL ${m[1]}(item => :row.id))`);
        input[am[1]] = valueOf(am[2]);
      }
    }
    const as = rest.match(new RegExp(`^\\s*AS\\s+(${NAME})\\s*$`, 'i'));
    if (rest.trim() && !as) fail(`CALL ${m[1]}: unexpected "${rest.trim().slice(0, 30)}"`);
    return [{ call: m[1], input, ...(as ? { as: as[1] } : {}) }];
  }
  // INSERT INTO t (cols) VALUES (values) [ON CONFLICT (cols) DO NOTHING | DO UPDATE SET ...] [AS name]
  if ((m = c.match(new RegExp(`^INSERT\\s+INTO\\s+(${NAME})\\s*`, 'i')))) {
    const table = m[1];
    const cols = group(c.slice(m[0].length));
    if (!cols) fail(`INSERT INTO ${table}: list the columns (INSERT INTO ${table} (a, b) VALUES (...))`);
    let rest = cols[1].trim();
    if (!/^VALUES\b/i.test(rest)) fail(`INSERT INTO ${table}: give VALUES (...); to add rows from a query, use FOR row IN (SELECT ...) DO INSERT ... END`);
    const vals = group(rest.slice(6));
    if (!vals) fail(`INSERT INTO ${table}: VALUES (...) is missing its values`);
    rest = vals[1].trim();
    const names = splitTop(cols[0], ',').map((x) => x.trim());
    const exprs = splitTop(vals[0], ',').map((x) => x.trim());
    if (names.length !== exprs.length) fail(`INSERT INTO ${table}: ${names.length} columns but ${exprs.length} values`);
    for (const n of names) if (!new RegExp(`^${NAME}$`).test(n)) fail(`INSERT INTO ${table}: ${n} is not a column name`);
    const values = Object.fromEntries(names.map((n, i) => [n, valueOf(exprs[i])]));
    const asAt = rest.match(new RegExp(`\\bAS\\s+(${NAME})\\s*$`, 'i'));
    const as = asAt ? asAt[1] : null;
    if (asAt) rest = rest.slice(0, asAt.index).trim();
    const named = as ? { as } : {};
    if (!rest) return [{ insert: table, values, ...named }];
    const oc = rest.match(/^ON\s+CONFLICT\s*/i);
    if (!oc) fail(`INSERT INTO ${table}: unexpected "${rest.slice(0, 30)}"`);
    const keys = group(rest.slice(oc[0].length));
    if (!keys) fail(`INSERT INTO ${table} ... ON CONFLICT: name the columns that find the row (ON CONFLICT (owner, product))`);
    const keyCols = splitTop(keys[0], ',').map((x) => x.trim());
    for (const k of keyCols) if (!names.includes(k)) fail(`ON CONFLICT (${k}): ${k} is not among the inserted columns`);
    const exprOf = Object.fromEntries(names.map((n, i) => [n, exprs[i]]));
    const find = `SELECT id FROM ${table} WHERE ${keyCols.map((k) => `${k} = ${sqlOf(exprOf[k])}`).join(' AND ')}`;
    const then = keys[1].trim();
    const found = tmp('found');
    if (/^DO\s+NOTHING$/i.test(then)) return [{ let: found, value: find }, { insert: table, values, if: `SELECT :${found} IS NULL`, ...named }];
    const up = then.match(/^DO\s+UPDATE\s+SET\s+([\s\S]+)$/i);
    if (!up) fail(`INSERT INTO ${table} ... ON CONFLICT (...): then DO NOTHING or DO UPDATE SET col = value`);
    const assigns = up[1].replace(new RegExp(`\\bexcluded\\.(${NAME})\\b`, 'gi'), (x, col) => { if (!(col in exprOf)) fail(`excluded.${col}: ${col} is not among the inserted columns`); return sqlOf(exprOf[col]); });
    return [{ let: found, value: find }, { insert: table, values, if: `SELECT :${found} IS NULL`, ...named }, { update: table, id: `:${found}`, set: setOf(assigns, table, `:${found}`), ...named }];
  }
  // UPDATE t SET ... WHERE id = value | WHERE condition
  if ((m = c.match(new RegExp(`^UPDATE\\s+(${NAME})\\s+SET\\s+`, 'i')))) {
    const table = m[1];
    let rest = c.slice(m[0].length);
    let asAt = rest.match(new RegExp(`\\s+AS\\s+(${NAME})\\s*$`, 'i'));
    if (asAt && findWord(rest, 'AS', asAt.index) !== asAt.index + asAt[0].search(/AS/i)) asAt = null;
    if (asAt) rest = rest.slice(0, asAt.index);
    const w = findWord(rest, 'WHERE');
    if (w < 0) fail(`UPDATE ${table}: give WHERE (WHERE id = :row.feature, or a condition)`);
    return writeWhere(table, rest.slice(w + 5).trim(), (idSql, id) => ({ update: table, id, set: setOf(rest.slice(0, w), table, idSql), ...(asAt ? { as: asAt[1] } : {}) }));
  }
  if ((m = c.match(new RegExp(`^DELETE\\s+FROM\\s+(${NAME})\\s*`, 'i')))) {
    const table = m[1];
    const rest = c.slice(m[0].length);
    if (!/^WHERE\b/i.test(rest)) fail(`DELETE FROM ${table}: give WHERE (WHERE id = :row.hold, or a condition)`);
    return writeWhere(table, rest.slice(5).trim(), (idSql, id) => ({ remove: table, id }));
  }
  if (SELECT_RE.test(c)) fail('a SELECT among other statements does nothing: name its value (LET total = (SELECT ...)), check it (REQUIRE ...) or give it back (RETURN (SELECT ...)); a rule that is only a query is CREATE RULE name AS SELECT ...');
  return fail(`"${c.slice(0, 40)}": a rule's statements are REQUIRE, LET, INSERT, UPDATE, DELETE, CALL, FOR ... DO ... END, IF ... THEN ... END and RETURN`);
}

// WHERE id = value writes one row; any other condition, each row it selects.
function writeWhere(table, cond, step) {
  const one = cond.match(/^id\s*=\s*([\s\S]+)$/i);
  if (one && findWord(one[1], 'AND') < 0 && findWord(one[1], 'OR') < 0) {
    const v = valueOf(one[1]);
    return [step(sqlOf(one[1]), typeof v === 'number' ? String(v) : v)];
  }
  const r = tmp('row');
  return [{ each: `SELECT id FROM ${table} WHERE ${cond}`, as: r, do: [step(`:${r}.id`, `:${r}.id`)] }];
}

// The statements of a body (also a test's "do", given as text), as steps.
export function compileSteps(text) {
  const q = splitTop(uncomment(String(text || '')), ';');
  return parseList(q, []).steps;
}

// CREATE RULE name [(inputs)] [RETURNS ROWS | ROW | VALUE]
//   [AFTER INSERT [OR UPDATE] [OR DELETE] ON table] AS body
// -> { name, on, rule }
export function compileRule(text) {
  seq = 0;
  const src = uncomment(String(text || '')).trim();
  const m = src.match(new RegExp(`^CREATE\\s+(?:OR\\s+REPLACE\\s+)?RULE\\s+([A-Za-z][A-Za-z0-9_-]*)\\s*`, 'i'));
  if (!m) fail('start with CREATE RULE name');
  let rest = src.slice(m[0].length);
  if (rest.startsWith('(')) rest = group(rest)[1].trim(); // the inputs, named for people: :name in the body
  const as = findWord(rest, 'AS');
  if (as < 0) fail(`CREATE RULE ${m[1]}: AS is missing before the body`);
  const head = rest.slice(0, as).trim();
  const body = rest.slice(as + 2).trim().replace(/;\s*$/, '');
  let on = null;
  let returns = 'rows';
  let h = head;
  const ret = h.match(/\bRETURNS\s+(ROWS|ROW|VALUE)\b/i);
  if (ret) { returns = ret[1].toLowerCase(); h = h.replace(ret[0], ' '); }
  const after = h.match(new RegExp(`\\bAFTER\\s+((?:INSERT|UPDATE|DELETE)(?:\\s*(?:OR|,)\\s*(?:INSERT|UPDATE|DELETE))*)\\s+ON\\s+(${NAME})\\b`, 'i'));
  if (after) {
    const events = [...new Set(after[1].toLowerCase().split(/\s*(?:or|,)\s*/))];
    on = { table: after[2], event: events.length === 1 ? events[0] : events };
    h = h.replace(after[0], ' ');
  }
  if (h.trim()) fail(`CREATE RULE ${m[1]}: "${h.trim().slice(0, 40)}" is not understood; before AS come RETURNS ROW | VALUE and AFTER INSERT [OR UPDATE] [OR DELETE] ON table`);
  if (!body) fail(`CREATE RULE ${m[1]}: the body after AS is empty`);
  const parts = splitTop(body, ';').filter((x) => x.trim());
  if (parts.length === 1 && SELECT_RE.test(parts[0])) {
    if (on) fail(`CREATE RULE ${m[1]}: a trigger writes; a SELECT alone changes nothing`);
    return { name: m[1], on: null, rule: { query: parts[0].trim(), ...(returns === 'row' ? { one: true } : returns === 'value' ? { value: true } : {}) } };
  }
  if (ret) fail(`CREATE RULE ${m[1]}: RETURNS is for a rule that is one SELECT; steps give back a value with RETURN`);
  const steps = compileSteps(body);
  if (!steps.length) fail(`CREATE RULE ${m[1]}: no statements`);
  return { name: m[1], on, rule: { steps } };
}

export const isRuleStatement = (code) => typeof code === 'string' && /^\s*(--[^\n]*\n\s*)*CREATE\s+(OR\s+REPLACE\s+)?RULE\b/i.test(code);

// A plan's CREATE RULE statements (in its functions' code blocks) become
// rules, with their triggers; the statement is kept as the rule's source.
export function compileRuleStatements(plan, code) {
  const problems = [];
  for (const f of plan?.functions || []) {
    if (!f || !isRuleStatement(code[f.slug])) continue;
    try {
      const out = compileRule(code[f.slug]);
      f.rule = out.rule;
      f.on = out.on;
      f.statement = code[f.slug].trim();
    } catch (e) {
      if (!(e instanceof RuleSqlError)) throw e;
      problems.push(`function ${f.slug}: CREATE RULE: ${e.message}`);
      delete f.rule;
    }
  }
  // A test's "do" written as statements becomes steps.
  for (const [i, t] of (Array.isArray(plan?.tests) ? plan.tests : []).entries()) {
    if (!t || typeof t.do !== 'string') continue;
    try { t.do = compileSteps(t.do); } catch (e) {
      if (!(e instanceof RuleSqlError)) throw e;
      problems.push(`tests[${i}]${t.name ? ` (${t.name})` : ''}.do: ${e.message}`);
      t.do = [];
    }
  }
  return problems;
}

export const RULE_SQL_DOC = `# Rules: functions and triggers written as SQL

A function that is a query, or does a few writes (a checkout, an invoice from hours, a purchase order, a stock move), or a trigger that keeps a count, a total or a stock level right, is a rule: write it as one CREATE RULE statement in its function's code block, with no "rule" in its plan entry. The function keeps its slug, lowercase with dashes, in the plan and on the code block (<code slug="count-votes">CREATE RULE count_votes ...</code>, called as app.call('count-votes')); the name after CREATE RULE is only for reading. The engine runs it directly. Write JavaScript only for what a rule cannot say (text or date logic beyond SQL, email, building a CSV).

A query: CREATE RULE spent [RETURNS ROW | VALUE] AS SELECT SUM(amount) AS total FROM expenses WHERE day >= :from
  app.call('spent', {from: '2026-11-01'}) gives the rows (ROW: the first row; VALUE: its first column). Similar rows and "bought together" are queries too (similarity, ppr).

Statements, each ending with ";", run in order:
CREATE RULE checkout AS
  REQUIRE (SELECT COUNT(*) FROM cart WHERE owner = :user.id) > 0 ELSE 'Your cart is empty';
  LET total = (SELECT SUM(c.qty * p.price) FROM cart c JOIN products p ON p.id = c.product WHERE c.owner = :user.id);
  INSERT INTO orders (customer, status, total) VALUES (:user.id, 'new', :total) AS order;
  FOR line IN (SELECT c.id, c.product, c.qty, p.price, p.stock, p.name FROM cart c JOIN products p ON p.id = c.product WHERE c.owner = :user.id) DO
    REQUIRE :line.stock >= :line.qty ELSE 'Only :line.stock left of :line.name';
    INSERT INTO order_lines (order_id, product, qty, price) VALUES (:order.id, :line.product, :line.qty, :line.price);
    UPDATE products SET stock = stock - :line.qty WHERE id = :line.product;
    DELETE FROM cart WHERE id = :line.id;
  END;
  IF :total > 100 THEN
    UPDATE orders SET status = 'review' WHERE id = :order.id;
  END;
  RETURN :order.id;

- REQUIRE condition [ELSE 'message']: when false, the rule stops, writes nothing, and the person sees the message (:name in it is filled in).
- LET name = value: a name later statements use as :name. LET name = CALL fn(...) keeps what another function gives back.
- INSERT INTO t (cols) VALUES (values) [AS name]: AS names the new row (:order.id). Add or update in one: INSERT INTO cart (owner, product, qty) VALUES (:user.id, :product, 1) ON CONFLICT (owner, product) DO UPDATE SET qty = qty + excluded.qty (or DO NOTHING).
- UPDATE t SET col = value, ... WHERE id = value (one row), or WHERE any condition (each row it selects). A value may use the row's own columns (stock - :line.qty).
- DELETE FROM t WHERE id = value, or WHERE any condition.
- CALL fn(name => value, ...) [AS name]: another function of the app.
- FOR name IN (SELECT ...) DO statements END: once per row, the row as :name.col (at most 1000 rows).
- IF condition THEN statements [ELSE statements] END.
- RETURN value: what app.call gives back.
- A condition or value is SQL: :name (the call's input, a LET, an AS), :row.col, a literal, an expression (:qty * :price), or a (SELECT ...) whose first value it is.

Triggers: CREATE RULE count_votes AFTER INSERT OR DELETE ON votes AS
  UPDATE features SET votes = (SELECT COUNT(*) FROM votes WHERE feature = :row.feature) WHERE id = :row.feature;
  It runs after each such write of that table with :row (the row as stored; the old row for a delete), :table and :event; give no "on" in the plan entry. Recompute a count or total with a SELECT rather than adding one, so deletes and edits stay right.

- A rule with statements is one transaction: its SELECTs see the data as it was when it started (not its own writes: compute a total from the cart, not from the order lines it adds), its writes happen together at the end or not at all, and two runs never overlap, so REQUIRE really holds (nothing is sold twice). Triggers on its writes run after.
- :user.id is the signed-in person (also :user.name and :user.email; null when nobody is).
- A rule is checked when the version is built: the tables and columns it writes must exist.`;

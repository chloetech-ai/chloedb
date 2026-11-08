// CREATE SCREEN: a screen written as an SQL-shaped statement, compiled into
// a screen (JSON, with the patterns' "use" blocks) before it is checked and
// stored. A statement is a fraction of the screen's JSON, and the model
// writes SQL well. One clause per line; a clause whose parentheses or
// brackets are open goes on to the next lines.
//
//   CREATE SCREEN roster TITLE 'Roster'
//   LIST players
//     ORDER BY name
//     SEARCH name, parent_name
//     FILTER position IN ('keeper', 'defence', 'attack')
//     FORM (name 'Player name' REQUIRED, jersey '#' NUMBER, parent_email EMAIL, notes TEXTAREA)
//     COLUMNS (jersey '#', name 'Player', position BADGE)
//     KPI 'Players' = COUNT(*)
//     KPI 'Missing a contact' = COUNT(*) WHERE parent_email IS NULL
//
// Parts: LIST, BOARD, DETAIL, SUMMARY (the patterns), HEADING, TEXT, DATA,
// STATE, LET and BLOCKS (screen JSON, for what the rest cannot say).

const PARTS = ['LIST', 'BOARD', 'DETAIL', 'SUMMARY', 'HEADING', 'TEXT', 'DATA', 'STATE', 'LET', 'BLOCKS'];
const FIELD_TYPES = ['TEXT', 'TEXTAREA', 'NUMBER', 'DATE', 'TIME', 'DATETIME', 'SELECT', 'CHECKBOX', 'TAGS', 'EMAIL', 'URL', 'COLOR', 'FILE', 'RATING'];
const COLUMN_FORMATS = ['DATE', 'DATETIME', 'AGO', 'MONEY', 'NUMBER', 'PERCENT', 'STARS', 'BOOL'];
const NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

class StatementError extends Error {}
const fail = (line, msg) => { throw new StatementError(`line ${line}: ${msg}`); };

// Splits text into lines, joining a line whose ( [ { or quote is open with
// the lines after it. Comments (-- to the end of a line) are dropped.
function logicalLines(text) {
  const out = [];
  let cur = '';
  let start = 0;
  let depth = 0;
  let quote = null;
  const lines = String(text).replace(/\r/g, '').split('\n');
  lines.forEach((raw, i) => {
    let line = '';
    for (let j = 0; j < raw.length; j++) {
      const c = raw[j];
      if (quote) {
        line += c;
        if (c === quote) { if (raw[j + 1] === quote) { line += raw[++j]; } else quote = null; }
        continue;
      }
      if (c === '-' && raw[j + 1] === '-') break;
      if (c === "'" || c === '"' || c === '`') quote = c;
      else if ('([{'.includes(c)) depth++;
      else if (')]}'.includes(c)) depth--;
      line += c;
    }
    if (!cur.trim()) start = i + 1;
    cur = cur ? `${cur}\n${line}` : line;
    if (depth <= 0 && !quote) { if (cur.trim()) out.push({ n: start, text: cur.trim().replace(/;$/, '').trim() }); cur = ''; depth = 0; }
  });
  if (cur.trim()) out.push({ n: start, text: cur.trim() });
  return out;
}

// Splits s at top-level commas (not inside quotes or brackets).
function splitTop(s) {
  const out = [];
  let depth = 0;
  let quote = null;
  let cur = '';
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (quote) { cur += c; if (c === quote) { if (s[i + 1] === quote) cur += s[++i]; else quote = null; } continue; }
    if (c === "'" || c === '"') quote = c;
    else if ('([{'.includes(c)) depth++;
    else if (')]}'.includes(c)) depth--;
    if (c === ',' && depth === 0) { out.push(cur.trim()); cur = ''; continue; }
    cur += c;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

// 'it''s' -> it's; a number; true/false/null; else the word.
function literal(s) {
  const t = s.trim();
  if (/^'.*'$/s.test(t)) return t.slice(1, -1).replace(/''/g, "'");
  if (/^".*"$/s.test(t)) return t.slice(1, -1).replace(/""/g, '"');
  if (/^-?\d+(\.\d+)?$/.test(t)) return Number(t);
  if (/^(true|false)$/i.test(t)) return t.toLowerCase() === 'true';
  if (/^null$/i.test(t)) return null;
  return t;
}

// "( ... )" -> the inside; the rest after it.
function parens(s, line, what) {
  const t = s.trim();
  if (!t.startsWith('(')) fail(line, `${what}: give a list in parentheses`);
  let depth = 0;
  let quote = null;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (quote) { if (c === quote) { if (t[i + 1] === quote) i++; else quote = null; } continue; }
    if (c === "'" || c === '"') quote = c;
    else if (c === '(') depth++;
    else if (c === ')') { depth--; if (!depth) return { inside: t.slice(1, i), rest: t.slice(i + 1).trim() }; }
  }
  return fail(line, `${what}: a parenthesis is not closed`);
}

const words = (s) => s.trim().split(/\s+/).filter(Boolean);
const names = (s, line, what) => splitTop(s).map((x) => { if (!NAME.test(x)) fail(line, `${what}: ${JSON.stringify(x)} is not a column name`); return x; });

// name 'Label' TYPE REQUIRED DEFAULT v ('option', ...)
function fieldSpec(spec, line) {
  const m = spec.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(.*)$/s);
  if (!m) fail(line, `FORM: ${JSON.stringify(spec)} does not start with a column name`);
  const f = { name: m[1] };
  let rest = m[2];
  while (rest.trim()) {
    rest = rest.trim();
    let mm;
    if ((mm = rest.match(/^('(?:[^']|'')*'|"(?:[^"]|"")*")/))) { f.label = literal(mm[1]); rest = rest.slice(mm[1].length); continue; }
    if (rest.startsWith('(')) { const p = parens(rest, line, `FORM ${f.name}`); f.options = splitTop(p.inside).map(literal); if (!f.type) f.type = 'select'; rest = p.rest; continue; }
    if ((mm = rest.match(/^DEFAULT\s+('(?:[^']|'')*'|[^\s,]+)/i))) { f.value = literal(mm[1]); rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^PLACEHOLDER\s+('(?:[^']|'')*')/i))) { f.placeholder = literal(mm[1]); rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^(NOT\s+NULL|REQUIRED)\b/i))) { f.required = true; rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^TYPE\s+/i))) { rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^([A-Za-z]+)\b/)) && FIELD_TYPES.includes(mm[1].toUpperCase())) { f.type = mm[1].toLowerCase(); if (f.type === 'text') delete f.type; rest = rest.slice(mm[0].length); continue; }
    fail(line, `FORM ${f.name}: cannot read ${JSON.stringify(rest.slice(0, 40))}; a field is name 'Label' TYPE REQUIRED DEFAULT value ('option', ...), types ${FIELD_TYPES.join(', ')}`);
  }
  return f;
}

// key 'Label' FORMAT BADGE
function columnSpec(spec, line) {
  const m = spec.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(.*)$/s);
  if (!m) fail(line, `COLUMNS: ${JSON.stringify(spec)} does not start with a column name`);
  const c = { key: m[1] };
  let rest = m[2];
  while (rest.trim()) {
    rest = rest.trim();
    let mm;
    if ((mm = rest.match(/^('(?:[^']|'')*'|"(?:[^"]|"")*")/))) { c.label = literal(mm[1]); rest = rest.slice(mm[1].length); continue; }
    if ((mm = rest.match(/^BADGE\b/i))) { c.badge = true; rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^FORMAT\s+/i))) { rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^([A-Za-z]+)\b/)) && COLUMN_FORMATS.includes(mm[1].toUpperCase())) { c.format = mm[1].toLowerCase(); rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^=\s*(.+)$/s))) { c.value = `=row => ${mm[1].trim()}`; rest = ''; continue; }
    fail(line, `COLUMNS ${c.key}: cannot read ${JSON.stringify(rest.slice(0, 40))}; a column is name 'Label' FORMAT BADGE (formats ${COLUMN_FORMATS.join(', ')}), or name 'Label' = a JavaScript expression of row`);
  }
  return c;
}

const sqlStr = (s) => `'${String(s).replace(/'/g, "''")}'`;
const jsStr = (s) => JSON.stringify(String(s));

// Compiles one CREATE SCREEN statement: {slug, title, screen}.
export function compileStatement(text) {
  const lines = logicalLines(text);
  if (!lines.length) throw new StatementError('the statement is empty');
  const head = lines[0].text.match(/^CREATE\s+SCREEN\s+([a-z][a-z0-9-]*)(?:\s+TITLE\s+('(?:[^']|'')*'))?\s*$/i);
  if (!head) throw new StatementError(`line ${lines[0].n}: start with CREATE SCREEN <slug> TITLE 'Title'`);
  const screen = { state: {}, data: {}, let: {}, body: [] };
  const title = head[2] ? literal(head[2]) : null;
  let part = null;
  let kpiN = 0;
  const finish = () => {
    if (!part) return;
    const p = part;
    part = null;
    if (p.kind === 'LIST' || p.kind === 'BOARD' || p.kind === 'DETAIL' || p.kind === 'SUMMARY') {
      for (const need of p.needs || []) if (p.use[need] === undefined) fail(p.n, `${p.kind} ${p.use.table}: needs ${{ fields: 'FORM (...)', status: "BY <column> ('a', 'b')", group: 'GROUP BY <column>' }[need]}`);
      screen.body.push(p.use);
    }
  };
  const SIMPLE = /^(TITLE|NOUN|EMPTY|LABEL|NAME|BACK|LIMIT|MONEY|DONUT)\b/i;
  const CLAUSE = '(?:TITLE|NOUN|EMPTY|LABEL|NAME|BACK|LIMIT|MONEY|DONUT|WHERE|ORDER BY|GROUP BY|VALUE|SEARCH|FILTER|TOGGLE|FORM|COLUMNS|CARDS|KPI|META|BY|ON ROW|ACTION|SHOW|PICK|RELATED|QUERY)\\b';
  // Several clauses may share a line: split after a simple clause (TITLE
  // 'x' NOUN 'y'), and at two or more spaces before a clause word, except in
  // the clauses that hold their own WHERE or JSON.
  const OWN = /^(KPI|ACTION|RELATED|DATA|LET|STATE|BLOCKS|QUERY)\b/i;
  const split = [];
  for (const ln of lines.slice(1)) {
    const pieces = OWN.test(ln.text) ? [ln.text] : ln.text.split(new RegExp(`\\s{2,}(?=${CLAUSE})`, 'i'));
    for (let t of pieces) {
      while (SIMPLE.test(t)) {
        const m = t.match(new RegExp(`^([A-Za-z]+\\s+(?:'(?:[^']|'')*'|[A-Za-z0-9_-]+)|MONEY|DONUT)\\s+(${CLAUSE}[\\s\\S]*)$`, 'i'));
        if (!m) break;
        split.push({ n: ln.n, text: m[1] });
        t = m[2];
      }
      split.push({ n: ln.n, text: t.trim() });
    }
  }
  for (const { n, text: l } of split) {
    const kw = (l.match(/^([A-Za-z]+(?:\s+BY)?)\b/) || [])[1]?.toUpperCase().replace(/\s+/g, ' ');
    const rest = l.slice((l.match(/^[A-Za-z]+(?:\s+BY)?\b/) || [''])[0].length).trim();
    if (PARTS.includes(kw)) {
      finish();
      if (kw === 'HEADING') { screen.body.push({ h: literal(rest) }); continue; }
      if (kw === 'TEXT') { screen.body.push({ text: literal(rest), muted: true }); continue; }
      if (kw === 'DATA') {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+AS\s+([\s\S]+)$/i);
        if (!m) fail(n, 'DATA <name> AS SELECT ...');
        let sql = m[2].trim();
        if (sql.startsWith('(') && sql.endsWith(')')) sql = sql.slice(1, -1).trim();
        screen.data[m[1]] = sql;
        continue;
      }
      if (kw === 'STATE' || kw === 'LET') {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
        if (!m) fail(n, `${kw} <name> = ${kw === 'STATE' ? 'a JSON value' : 'a JavaScript expression'}`);
        if (kw === 'LET') screen.let[m[1]] = `=${m[2].trim()}`;
        else { try { screen.state[m[1]] = JSON.parse(m[2]); } catch { screen.state[m[1]] = literal(m[2]); } }
        continue;
      }
      if (kw === 'BLOCKS') {
        let blocks;
        try { blocks = JSON.parse(rest); } catch (e) { fail(n, `BLOCKS takes a JSON list of screen blocks: ${e.message}`); }
        screen.body.push(...(Array.isArray(blocks) ? blocks : [blocks]));
        continue;
      }
      const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)(?:\s+AS\s+([A-Za-z_][A-Za-z0-9_]*))?\s*$/i);
      if (!m) fail(n, `${kw} <table> [AS <key>]`);
      const use = { use: { LIST: 'crud', BOARD: 'board', DETAIL: 'detail', SUMMARY: 'summary' }[kw], table: m[1] };
      if (m[2]) use.key = m[2];
      part = { kind: kw, n, use, needs: { LIST: ['fields'], BOARD: ['status'], DETAIL: ['fields'], SUMMARY: ['group'] }[kw], kpis: [] };
      continue;
    }
    if (!part) fail(n, `${JSON.stringify(l.slice(0, 40))} does not start a part (${PARTS.join(', ')})`);
    const u = part.use;
    const key = u.key || (part.kind === 'SUMMARY' ? `${u.table}_by` : u.table);
    switch (kw) {
      case 'TITLE': u.title = literal(rest); break;
      case 'NOUN': u.noun = literal(rest); break;
      case 'EMPTY': u.empty = literal(rest); break;
      case 'WHERE': u.where = rest; break;
      case 'ORDER BY': u.order = rest; break;
      case 'QUERY': u.sql = rest.replace(/^\(([\s\S]*)\)$/, '$1').trim(); break;
      case 'SEARCH': u.search = names(rest, n, 'SEARCH'); break;
      case 'FILTER': {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+(?:IN\s+)?([\s\S]+)$/i);
        if (!m) fail(n, "FILTER <column> IN ('a', 'b')");
        u.filter = { column: m[1], options: splitTop(parens(m[2], n, 'FILTER').inside).map(literal) };
        break;
      }
      case 'TOGGLE': { const w = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(.*)$/); if (!w) fail(n, "TOGGLE <bool column> ['Label']"); u.toggle = w[1]; if (w[2].trim()) u.toggleLabel = literal(w[2]); break; }
      case 'FORM': {
        let r = rest;
        const pos = r.match(/^(SIDE|TOP|NONE)\b/i);
        if (pos) { u.form = pos[1].toLowerCase(); r = r.slice(pos[0].length).trim(); }
        if (r) { const p = parens(r, n, 'FORM'); u.fields = splitTop(p.inside).map((f) => fieldSpec(f, n)); const after = p.rest.match(/^(SIDE|TOP|NONE)\b/i); if (after) u.form = after[1].toLowerCase(); }
        if (part.kind !== 'LIST' && u.form) delete u.form;
        if (!u.fields && u.form === 'none') u.fields = [{ name: 'id' }];
        break;
      }
      case 'COLUMNS': u.columns = splitTop(parens(rest, n, 'COLUMNS').inside).map((c) => columnSpec(c, n)); break;
      case 'SHOW': u.show = splitTop(parens(rest, n, 'SHOW').inside).map((c) => columnSpec(c, n)); break;
      case 'PICK': u.list = splitTop(parens(rest, n, 'PICK').inside).map((c) => columnSpec(c, n)); break;
      case 'CARDS': {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)(?:\s+SUB\s+(.+))?$/i);
        if (!m) fail(n, 'CARDS <title column> [SUB col, col]');
        const sub = m[2] ? names(m[2], n, 'CARDS SUB') : [];
        u.card = [{ h: `=row.${m[1]}`, level: 3 }, ...(sub.length ? [{ text: `=[${sub.map((c) => `row.${c}`).join(', ')}].filter(v => v != null && v !== '').join(' · ')`, muted: true }] : [])];
        break;
      }
      case 'KPI': {
        const m = rest.match(/^('(?:[^']|'')*')\s*=\s*([\s\S]+?)(?:\s+WHERE\s+([\s\S]+?))?(\s+MONEY)?\s*$/i);
        if (!m) fail(n, "KPI 'Label' = COUNT(*) | SUM(col) | AVG(col) ... [WHERE condition] [MONEY]");
        const name = `${key}_k${kpiN++}`;
        const conds = [u.where, m[3]].filter(Boolean).map((c) => `(${c})`).join(' AND ');
        screen.data[name] = `SELECT ${m[2]} AS v FROM ${u.table}${conds ? ` WHERE ${conds}` : ''}`;
        const fmt = m[4] ? 'money' : 'number';
        u.kpis = [...(u.kpis || []), { label: literal(m[1]), value: `=app.fmt.${fmt}(Number((${name}[0] || {}).v) || 0)` }];
        break;
      }
      case 'ON': {
        const m = rest.match(/^ROW\s+OPEN\s+([a-z][a-z0-9-]*)$/i);
        if (!m) fail(n, 'ON ROW OPEN <view slug>');
        u.onRow = `=row => app.open(${jsStr(m[1])}, {id: row.id})`;
        break;
      }
      case 'ACTION': {
        const m = rest.match(/^('(?:[^']|'')*')\s+(OPEN\s+([a-z][a-z0-9-]*)|SET\s+([\s\S]+)|DELETE)$/i);
        if (!m) fail(n, "ACTION 'Label' OPEN <view slug> | SET col = value, ... | DELETE");
        let doIt;
        if (m[3]) doIt = `=row => app.open(${jsStr(m[3])}, {id: row.id})`;
        else if (m[4]) {
          const sets = splitTop(m[4]).map((x) => { const mm = x.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/); if (!mm) fail(n, `ACTION SET: ${JSON.stringify(x)} is not col = value`); return [mm[1], literal(mm[2])]; });
          doIt = `=row => app.update(${jsStr(u.table)}, row.id, ${JSON.stringify(Object.fromEntries(sets.map(([k, v]) => [k, v === 'TODAY' || v === 'today' ? '@today' : v]))).replace(/"@today"/g, 'app.today()')})`;
        } else doIt = `=row => app.remove(${jsStr(u.table)}, row.id)`;
        u.actions = [...(u.actions || []), { label: literal(m[1]), class: 'small ghost', do: doIt }];
        break;
      }
      case 'BY': case 'STATUS': {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*([\s\S]*)$/);
        if (!m) fail(n, "BY <status column> ('new', 'doing', 'done')");
        u.status = m[1];
        if (m[2].trim()) u.statuses = splitTop(parens(m[2], n, 'BY').inside).map(literal);
        break;
      }
      case 'NAME': u.name = rest; if (!NAME.test(rest)) fail(n, 'NAME <column>'); break;
      case 'META': u.meta = names(rest, n, 'META'); break;
      case 'BACK': u.back = rest; break;
      case 'RELATED': {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+BY\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:TITLE\s+('(?:[^']|'')*'))?\s*(?:COLUMNS\s+([\s\S]+?))?\s*(?:ORDER BY\s+(.+))?$/i);
        if (!m) fail(n, "RELATED <table> BY <its column holding this row's id> [TITLE 'Title'] [COLUMNS (...)] [ORDER BY ...]");
        u.related = [...(u.related || []), { table: m[1], by: m[2], ...(m[3] ? { title: literal(m[3]) } : {}), ...(m[4] ? { columns: splitTop(parens(m[4], n, 'RELATED COLUMNS').inside).map((c) => columnSpec(c, n)) } : {}), ...(m[5] ? { order: m[5] } : {}) }];
        break;
      }
      case 'GROUP BY': u.group = rest; break;
      case 'VALUE': u.value = rest; break;
      case 'LABEL': u.label = literal(rest); break;
      case 'MONEY': u.format = 'money'; break;
      case 'DONUT': u.chart = 'donut'; break;
      case 'LIMIT': u.limit = Number(rest) || 20; break;
      default: fail(n, `${part.kind} does not take ${JSON.stringify(l.slice(0, 30))}`);
    }
  }
  finish();
  for (const k of ['state', 'data', 'let']) if (!Object.keys(screen[k]).length) delete screen[k];
  return { slug: head[1], title, screen };
}

// The plan's views whose code is a CREATE SCREEN statement become screens.
// Returns problems.
export function compileStatements(plan, code) {
  const problems = [];
  for (const v of plan?.views || []) {
    if (!v || typeof code[v.slug] !== 'string' || !/^\s*CREATE\s+SCREEN\b/i.test(code[v.slug])) continue;
    try {
      const out = compileStatement(code[v.slug]);
      v.screen = out.screen;
      v.statement = true;
      if (!v.title && out.title) v.title = out.title;
      delete code[v.slug];
    } catch (e) {
      if (!(e instanceof StatementError)) throw e;
      // The statement stays as the view's code, so a writer asked to fix it
      // sees what it wrote.
      problems.push(`view ${v.slug}: CREATE SCREEN ${e.message}`);
      delete v.screen;
    }
  }
  return problems;
}

export const STATEMENT_DOC = `# CREATE SCREEN: screens as statements

Write each screen view as one CREATE SCREEN statement in its code block (<code slug="the-slug">CREATE SCREEN ...</code>), and give no "screen" in its plan entry. A statement is SQL-shaped and far shorter than a screen's JSON; it compiles into the screen patterns and blocks described above. One clause per line (a clause whose parentheses are open goes on). Parts, each starting a line, in the order they show:

CREATE SCREEN <slug> TITLE 'Title'
LIST <table> [AS key]          -- the crud pattern: rows, add/edit form, search, filter, toggle, delete
  TITLE 'Heading'  NOUN 'guest'  EMPTY 'No guests yet'
  WHERE <SQL condition>  ORDER BY <SQL>  QUERY (SELECT ... when the rows need a join or computed columns)
  SEARCH col, col                       FILTER col IN ('a', 'b')        TOGGLE <bool col> ['Done']
  FORM [SIDE | NONE] (col 'Label' NUMBER REQUIRED DEFAULT 0, col SELECT ('a', 'b'), ...)   after the label, one type word: ${FIELD_TYPES.join(' ')}
  COLUMNS (col 'Label' DATE, col 'Label' MONEY BADGE, col 'Label' = <JavaScript of row>, ...)   after the label, one format word: ${COLUMN_FORMATS.join(' ')}, and BADGE
  CARDS <title col> [SUB col, col]      (cards instead of a table)
  KPI 'Label' = COUNT(*) | SUM(col) | AVG(col) [WHERE condition] [MONEY]   (one line each; SQL over the table)
  ON ROW OPEN <view slug>   ACTION 'Label' OPEN <view slug> | SET col = value, ... | DELETE
BOARD <table> [AS key]         -- the board pattern
  BY <status col> ('new', 'doing', 'done')  NAME <title col>  META col, col  FORM (...)  TITLE  NOUN  WHERE  ORDER BY
DETAIL <table> [AS key]        -- the detail pattern: one row (opened with app.open(slug, {id})), edit, related rows
  NAME <heading col>  FORM (...)  SHOW (columns)  PICK (columns of the pick list)  BACK <view slug>  TITLE  NOUN
  RELATED <table> BY <its column holding the id> [TITLE 'Title'] [COLUMNS (...)] [ORDER BY ...]   (one line each)
SUMMARY <table> [AS key]       -- the summary pattern: a total and a bar (or donut) per group
  GROUP BY <SQL>  VALUE SUM(col)  LABEL 'Spent'  WHERE <SQL>  MONEY  DONUT  LIMIT 20  TITLE 'Heading'
HEADING 'text'   TEXT 'text'
DATA <name> AS SELECT ...      -- more data for BLOCKS and LET, by name
STATE <name> = <JSON>          LET <name> = <JavaScript expression>
BLOCKS [ ...screen blocks as JSON... ]   -- anything the parts cannot say (charts, custom lists, code blocks); they see data, lets and the patterns' names

Example:
CREATE SCREEN guests TITLE 'Guests'
LIST guests
  TITLE 'Guests'  NOUN 'guest'
  ORDER BY name
  SEARCH name
  FILTER rsvp IN ('yes', 'no', 'waiting')
  FORM (name REQUIRED, rsvp 'RSVP' SELECT ('yes', 'no', 'waiting') DEFAULT 'waiting', meal SELECT ('fish', 'veg'), table_no 'Table' NUMBER)
  COLUMNS (name, rsvp 'RSVP' BADGE, meal, table_no 'Table')
  KPI 'Coming' = COUNT(*) WHERE rsvp = 'yes'
  KPI 'Waiting' = COUNT(*) WHERE rsvp = 'waiting'
SUMMARY guests AS meals
  TITLE 'Meals'  GROUP BY meal  WHERE rsvp = 'yes'

A screen that is mostly custom (a game, a chart of its own) may be a CREATE SCREEN with a BLOCKS part, or a JSON "screen" in the plan as described above. To change a screen the app has, use a "patch" as described above (the app shows screens as JSON).`;

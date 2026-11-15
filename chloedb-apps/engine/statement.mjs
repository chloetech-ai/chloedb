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

import { parseLayout, LAYOUT_SHORT, LAYOUT_NAMES } from './quality.mjs';

// The newer patterns: a part names its table, then each clause sets one of
// the pattern's parameters: [parameter, kind of value].
const MORE = {
  CHECKS: { use: 'checks', need: { items: 'ITEMS <table of the things to check off>', item: "ITEM <column holding the thing's id>" },
    clauses: { ITEMS: ['items', 'name'], ITEM: ['item', 'name'], DAY: ['day', 'name'], NAME: ['name', 'name'], DAYS: ['days', 'num'], WHERE: ['itemWhere', 'sql'], 'ORDER BY': ['itemOrder', 'sql'], ADD: ['add', 'flag'], NOUN: ['noun', 'str'], TITLE: ['title', 'str'], SKIP: ['skip', 'name'] } },
  WEEK: { use: 'week', need: { items: 'ITEMS <table of the things to tick>', item: "ITEM <column holding the thing's id>" },
    clauses: { ITEMS: ['items', 'name'], ITEM: ['item', 'name'], DAY: ['day', 'name'], NAME: ['name', 'name'], SUB: ['sub', 'name'], LABEL: ['label', 'str'], EXTRA: ['extra', 'js'], TITLE: ['title', 'str'] } },
  CALENDAR: { use: 'calendar', need: { fields: 'FORM (...)' },
    clauses: { DATE: ['date', 'name'], NAME: ['name', 'name'], TIME: ['time', 'name'], PLACE: ['place', 'name'], COLOR: ['color', 'name'], FORM: ['fields', 'fields'], WHERE: ['where', 'sql'], TITLE: ['title', 'str'], NOUN: ['noun', 'str'] } },
  FLASHCARDS: { use: 'flashcards',
    clauses: { FRONT: ['front', 'name'], BACK: ['back', 'name'], DUE: ['due', 'name'], INTERVAL: ['interval', 'name'], MASTERED: ['mastered', 'num'], WHERE: ['where', 'sql'], PARAM: ['params', 'param'], TITLE: ['title', 'str'] } },
  QUIZ: { use: 'quiz',
    clauses: { QUESTION: ['question', 'name'], CHOICES: ['choices', 'name'], ANSWER: ['answer', 'name'], EXPLANATION: ['explanation', 'name'], COUNT: ['count', 'num'], RESULTS: ['results', 'name'], EXTRA: ['resultExtra', 'js'], WHERE: ['where', 'sql'], PARAM: ['params', 'param'], TITLE: ['title', 'str'] } },
  FIND: { use: 'search', need: { text: 'TEXT <text column>' },
    clauses: { TEXT: ['text', 'name'], MEANING: ['meaning', 'name'], NAME: ['name', 'name'], SUB: ['sub', 'names'], SHOW: ['show', 'cols'], LIMIT: ['limit', 'num'], PLACEHOLDER: ['placeholder', 'str'], WHERE: ['where', 'sql'], PARAM: ['params', 'param'], TITLE: ['title', 'str'] } },
  DASHBOARD: { use: 'dashboard',
    clauses: { DATE: ['date', 'name'], DAY: ['date', 'day'], VALUE: ['value', 'sql'], LABEL: ['label', 'str'], FORMAT: ['format', 'word'], DAYS: ['days', 'num'], BY: ['group', 'name'], 'GROUP BY': ['group', 'name'], WHERE: ['where', 'sql'], RECENT: ['recent', 'cols'], TITLE: ['title', 'str'] } },
  OFFERS: { use: 'offers', need: { left: 'LEFT <column of how many are left>' },
    clauses: { NAME: ['name', 'name'], TEXT: ['text', 'name'], PRICE: ['price', 'name'], IMAGE: ['image', 'name'], LEFT: ['left', 'name'], DATE: ['date', 'name'], WHERE: ['where', 'sql'], 'ORDER BY': ['order', 'sql'], SOON: ['soon', 'num'], OUT: ['out', 'str'], ACTION: ['action', 'str'], OPEN: ['open', 'slug'], CALL: ['call', 'slug'], PAY: ['pay', 'name'], CHARGE: ['charge', 'name'], PAID: ['paid', 'name'], QTY: ['qty', 'flag'], COUNT: ['count', 'flag'], DONE: ['done', 'str'], LOOK: ['look', 'word'], TITLE: ['title', 'str'], EYEBROW: ['eyebrow', 'str'], ID: ['id', 'slug'] } },
  LANES: { use: 'lanes', need: { by: 'BY <column whose value is the lane>' },
    clauses: { BY: ['by', 'name'], LANES: ['lanes', 'str'], NAME: ['name', 'name'], DETAIL: ['detail', 'name'], SUB: ['sub', 'name'], FLAGS: ['flags', 'sql'], WHERE: ['where', 'sql'], 'ORDER BY': ['order', 'sql'], ACTION: ['action', 'action'], EMPTY: ['empty', 'str'], TITLE: ['title', 'str'] } },
  CALLOUTS: { use: 'callouts', need: { head: 'HEAD <column with each card\'s title>' },
    clauses: { HEAD: ['head', 'name'], CHIPS: ['chips', 'list'], JOIN: ['join', 'str'], TEXT: ['text', 'name'], TIP: ['tip', 'name'], TONE: ['tone', 'sql'], LABEL: ['label', 'sql'], WHERE: ['where', 'sql'], 'ORDER BY': ['order', 'sql'], SUB: ['sub', 'text'], EMPTY: ['empty', 'str'], TITLE: ['title', 'str'] } },
  ASK: { use: 'ask', bare: 'title', need: { over: 'OVER (SELECT ... the rows it reads)', prompt: "PROMPT 'the question'" },
    clauses: { OVER: ['over', 'sql'], PROMPT: ['prompt', 'str'], BUTTON: ['button', 'str'], NOTE: ['note', 'str'], INPUT: ['input', 'str'], PLACEHOLDER: ['placeholder', 'str'], WEB: ['web', 'flag'], AS: ['key', 'word'] } },
  HEADER: { use: 'header', bare: 'title',
    clauses: { SUB: ['sub', 'text'], PRIMARY: ['primary', 'text'], OPEN: ['open', 'slug'], CALL: ['call', 'slug'], SECONDARY: ['secondary', 'text'], SECONDARYOPEN: ['secondaryOpen', 'slug'] } },
  STEP: { use: 'step', need: { progress: 'PROGRESS <table of the ones done>' },
    clauses: { PROGRESS: ['progress', 'name'], ITEM: ['item', 'name'], CONTENT: ['content', 'slug'], BODY: ['body', 'name'], NAME: ['name', 'name'], 'ORDER BY': ['order', 'sql'], BACK: ['back', 'slug'], NOUN: ['noun', 'str'], DONE: ['done', 'str'] } },
  LESSONS: { use: 'lessons', need: { progress: 'PROGRESS <table of lessons done>' },
    clauses: { NEEDS: ['needs', 'sql'], LOCKED: ['locked', 'str'], UNLOCK: ['unlock', 'slug'], UNLOCKLABEL: ['unlockLabel', 'str'], PROGRESS: ['progress', 'name'], LESSON: ['lesson', 'name'], NAME: ['name', 'name'], TEXT: ['text', 'name'], MINUTES: ['minutes', 'name'], 'ORDER BY': ['order', 'sql'], WHERE: ['where', 'sql'], PARAM: ['params', 'param'], OPEN: ['open', 'slug'], CERTIFICATE: ['certificate', 'slug'], NOUN: ['noun', 'str'], TITLE: ['title', 'str'], ID: ['id', 'slug'] } },
  VARIANCE: { use: 'variance', need: { period: 'PERIOD <column or SQL naming each period>', value: 'VALUE <column, or an aggregate with GROUP BY>' },
    clauses: { PERIOD: ['period', 'sql'], VALUE: ['value', 'sql'], COMPARE: ['compare', 'sql'], AGAINST: ['compareLabel', 'str'], CHART: ['chart', 'word'], 'GROUP BY': ['group', 'sql'], 'ORDER BY': ['order', 'sql'], LABEL: ['label', 'str'], NAME: ['periodLabel', 'str'], FORMAT: ['format', 'word'], WHERE: ['where', 'sql'], LAST: ['last', 'num'], FILE: ['file', 'str'], TITLE: ['title', 'str'] } },
  NEXT: { use: 'next', need: { date: 'DATE <column with the day>' },
    clauses: { DATE: ['date', 'name'], NAME: ['name', 'name'], TIME: ['time', 'name'], PLACE: ['place', 'name'], TEXT: ['text', 'name'], BADGE: ['badge', 'name'], WHERE: ['where', 'sql'], EYEBROW: ['eyebrow', 'str'], ACTION: ['action', 'str'], OPEN: ['open', 'slug'], CALL: ['call', 'slug'], TITLE: ['title', 'str'], MORE: ['more', 'num'], EMPTY: ['empty', 'str'] } },
  DUE: { use: 'due', need: { due: 'DUE <column with the day it falls due>' },
    clauses: { DUE: ['due', 'name'], NAME: ['name', 'name'], SUB: ['sub', 'names'], SOON: ['soon', 'num'], WHERE: ['where', 'sql'], TITLE: ['title', 'str'], NOUN: ['noun', 'str'], LABEL: ['label', 'str'], OPEN: ['open', 'slug'], ACTION: ['action', 'str'], SET: ['set', 'js'], CALL: ['call', 'slug'], EMPTY: ['empty', 'str'] } },
  LEND: { use: 'lending', need: { items: 'ITEMS <table of the things lent>' },
    clauses: { ITEMS: ['items', 'name'], ITEM: ['item', 'name'], NAME: ['name', 'name'], SUB: ['sub', 'name'], HOLDER: ['holder', 'name'], PERSON: ['person', 'flag'], OUT: ['out', 'name'], DUE: ['due', 'name'], BACK: ['back', 'name'], DAYS: ['days', 'num'], TITLE: ['title', 'str'], NOUN: ['noun', 'str'], LABEL: ['label', 'str'] } },
  PLAN: { use: 'planner', need: { items: 'ITEMS <table of the things to pick from>' },
    clauses: { ITEMS: ['items', 'name'], DAY: ['day', 'name'], ITEM: ['item', 'name'], NAME: ['name', 'name'], PARTS: ['parts', 'name'], OF: ['of', 'name'], PART: ['part', 'name'], QTY: ['qty', 'name'], UNIT: ['unit', 'name'], TITLE: ['title', 'str'], PICK: ['pick', 'str'], TOTALS: ['totals', 'str'] } },
  REPORT: { use: 'report', bare: 'title', need: { sections: "SECTION 'Title' (SELECT ...)" },
    clauses: { SECTION: ['sections', 'section'], SUB: ['sub', 'text'] } },
  TEAM: { use: 'team', need: { things: 'THINGS <table of what people are put on>' },
    clauses: { THINGS: ['things', 'name'], PERSON: ['person', 'name'], THING: ['thing', 'name'], NAME: ['name', 'name'], ROLE: ['role', 'name'], ROLES: ['roles', 'strs'], TITLE: ['title', 'str'], LABEL: ['label', 'str'] } },
  PRICES: { use: 'prices',
    clauses: { NAME: ['name', 'name'], PRICE: ['price', 'name'], NOTE: ['note', 'name'], GROUP: ['group', 'name'], BADGE: ['badge', 'name'], UNIT: ['unit', 'str'], COLUMNS: ['columns', 'cols'], WHERE: ['where', 'sql'], 'ORDER BY': ['order', 'sql'], TITLE: ['title', 'str'], EYEBROW: ['eyebrow', 'str'], OPEN: ['open', 'slug'], EMPTY: ['empty', 'str'] } },
  TIMETABLE: { use: 'timetable',
    clauses: { DAY: ['day', 'name'], TIME: ['time', 'name'], MINUTES: ['minutes', 'name'], NAME: ['name', 'name'], SUB: ['sub', 'name'], PLACE: ['place', 'name'], SPACES: ['spaces', 'name'], BOOKINGS: ['bookings', 'name'], OF: ['of', 'name'], DATE: ['date', 'name'], ACTION: ['action', 'str'], OPEN: ['open', 'slug'], CALL: ['call', 'slug'], TITLE: ['title', 'str'] } },
  SLOTS: { use: 'slots',
    clauses: { START: ['start', 'name'], END: ['end', 'name'], OPEN: ['open', 'str'], CLOSE: ['close', 'str'], STEP: ['step', 'num'], LENGTH: ['length', 'num'], CLOSED: ['closed', 'nums'], DAYS: ['days', 'num'], RESOURCE: ['resource', 'name'], RESOURCES: ['resources', 'name'], LABEL: ['label', 'name'], VIEW: ['view', 'slug'], CALL: ['call', 'slug'], TITLE: ['title', 'str'], NOTE: ['note', 'str'] } },
  INBOX: { use: 'inbox',
    clauses: { NAME: ['name', 'name'], CONTACT: ['contact', 'names'], TEXT: ['text', 'name'], STATUS: ['status', 'name'], STATES: ['states', 'strs'], SUB: ['sub', 'names'], TITLE: ['title', 'str'], NOUN: ['noun', 'str'] } },
  STANDINGS: { use: 'standings',
    clauses: { BY: ['by', 'name'], VALUE: ['value', 'sql'], SUB: ['sub', 'sql'], UNIT: ['subword', 'str'], WHERE: ['where', 'sql'], QUERY: ['query', 'sql'], ME: ['me', 'js'], FORMAT: ['format', 'word'], LIMIT: ['limit', 'num'], TITLE: ['title', 'str'] } },
  DOCUMENT: { use: 'document',
    clauses: { LABEL: ['label', 'str'], NUMBER: ['number', 'name'], DATE: ['date', 'name'], TO: ['to', 'name'], BY: ['from', 'text'], LINES: ['lines', 'name'], OF: ['of', 'name'], ITEM: ['item', 'name'], QTY: ['qty', 'name'], PRICE: ['price', 'name'], TAX: ['tax', 'num'], TOTAL: ['total', 'name'], NOTE: ['note', 'name'], BACK: ['back', 'slug'] } },
  GALLERY: { use: 'photos',
    clauses: { FILE: ['file', 'name'], CAPTION: ['caption', 'name'], ALBUM: ['album', 'name'], WHERE: ['where', 'sql'], UPLOAD: ['upload', 'flag'], LOOK: ['look', 'word'], TITLE: ['title', 'str'], EYEBROW: ['eyebrow', 'str'], EMPTY: ['empty', 'str'] } },
  TREND: { use: 'trend',
    clauses: { DATE: ['date', 'name'], DAY: ['date', 'name'], VALUE: ['value', 'sql'], LABEL: ['label', 'str'], PER: ['per', 'word'], LAST: ['last', 'num'], FORMAT: ['format', 'word'], WHERE: ['where', 'sql'], LINE: ['line', 'flag'], TITLE: ['title', 'str'] } },
};
const PARTS = ['LIST', 'BOARD', 'DETAIL', 'SUMMARY', ...Object.keys(MORE), 'HEADING', 'TEXT', 'DATA', 'STATE', 'LET', 'BLOCKS', 'PAGE', 'LAYOUT', 'PHOTOS', 'FOR'];
const FIELD_TYPES = ['TEXT', 'TEXTAREA', 'NUMBER', 'DATE', 'TIME', 'DATETIME', 'SELECT', 'CHECKBOX', 'TAGS', 'EMAIL', 'URL', 'COLOR', 'FILE', 'RATING', 'PERSON', 'PEOPLE'];
const COLUMN_FORMATS = ['DATE', 'DATETIME', 'AGO', 'DAYS', 'WEEKDAY', 'MONTH', 'MONEY', 'NUMBER', 'PERCENT', 'PCT', 'HOURS', 'DURATION', 'CLOCK', 'STARS', 'BOOL'];
// How a big number reads (KPI ... MONEY): the column formats that fit a number.
const KPI_FORMATS = ['MONEY', 'NUMBER', 'PERCENT', 'PCT', 'HOURS', 'DURATION', 'CLOCK'];
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
        // In a double-quoted (JSON) string a backslash escapes what follows.
        if (quote === '"' && c === '\\' && j + 1 < raw.length) { line += raw[++j]; continue; }
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
    if (rest.startsWith('(')) {
      const p = parens(rest, line, `FORM ${f.name}`);
      const inner = p.inside.trim().replace(/^\(([\s\S]*)\)$/, '$1').trim();
      if (/^SELECT\b/i.test(inner)) {
        // (SELECT id, name FROM flats): the rows of a table, said as FROM
        const q = inner.match(/^SELECT\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*,\s*([A-Za-z_][A-Za-z0-9_]*))?\s+FROM\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:ORDER\s+BY\s+[\s\S]*)?$/i);
        if (!q) fail(line, `FORM ${f.name}: a choice among a table's rows is ${f.name} FROM <table> (<value col>, <label col>)`);
        f.from = { table: q[3], value: q[1], label: q[2] || q[1] };
        f.type = 'select';
      } else {
        f.options = splitTop(p.inside).map(literal);
        if (!f.type) f.type = 'select';
      }
      rest = p.rest;
      continue;
    }
    if ((mm = rest.match(/^DEFAULT\s+('(?:[^']|'')*'|[^\s,]+)/i))) { f.value = literal(mm[1]); rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^PLACEHOLDER\s+('(?:[^']|'')*')/i))) { f.placeholder = literal(mm[1]); rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^(NOT\s+NULL|REQUIRED)\b/i))) { f.required = true; rest = rest.slice(mm[0].length); continue; }
    if ((mm = rest.match(/^TYPE\s+/i))) { rest = rest.slice(mm[0].length); continue; }
    // FROM table [(value col [, label col])]: a pick list of that table's rows (default (id, name))
    if ((mm = rest.match(/^FROM\s+([A-Za-z_][A-Za-z0-9_]*)\s*/i))) {
      f.from = { table: mm[1], value: 'id', label: 'name' };
      rest = rest.slice(mm[0].length);
      if (rest.startsWith('(')) {
        const p = parens(rest, line, `FORM ${f.name} FROM ${mm[1]}`);
        const cols = splitTop(p.inside).map((x) => x.trim()).filter(Boolean);
        if (!cols.length || cols.length > 2 || !cols.every((x) => NAME.test(x))) fail(line, `FORM ${f.name} FROM ${mm[1]} (value column [, label column])`);
        f.from.value = cols[0];
        f.from.label = cols[1] || cols[0];
        rest = p.rest;
      }
      f.type = 'select';
      continue;
    }
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
    // = an expression of row; a function of the row (= r => ...) is taken as it is.
    if ((mm = rest.match(/^=\s*(.+)$/s))) { const e = mm[1].trim(); c.value = /^(\(\s*[A-Za-z_$][\w$]*\s*\)|[A-Za-z_$][\w$]*)\s*=>/.test(e) ? `=${e}` : `=row => ${e}`; rest = ''; continue; }
    fail(line, `COLUMNS ${c.key}: cannot read ${JSON.stringify(rest.slice(0, 40))}; a column is name 'Label' FORMAT BADGE (formats ${COLUMN_FORMATS.join(', ')}), or name 'Label' = a JavaScript expression of row`);
  }
  return c;
}

// KPI 'Label' = <aggregate> [FROM table] [WHERE condition] [format]
function kpiOf(rest, line, from = false) {
  const m = rest.match(new RegExp(`^('(?:[^']|'')*')\\s*=\\s*([\\s\\S]+?)(?:\\s+FROM\\s+([A-Za-z_][A-Za-z0-9_]*))?(?:\\s+WHERE\\s+([\\s\\S]+?))?(?:\\s+(${KPI_FORMATS.join('|')}))?\\s*$`, 'i'));
  if (!m || (from && !m[3])) fail(line, `KPI 'Label' = COUNT(*) | SUM(col) | AVG(col) ...${from ? ' FROM <table>' : ''} [WHERE condition] [${KPI_FORMATS.join(' | ')}]`);
  return { label: literal(m[1]), value: m[2].trim(), table: m[3], where: m[4], format: (m[5] || 'NUMBER').toLowerCase() };
}

// A query that names :today gets it from the screen.
function withDays(sql) {
  return /:today\b/.test(sql) ? { sql, params: { today: '=today' } } : sql;
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
  let photos = null;
  let audience = null;
  let kpiN = 0;
  let kpiRow = null;
  const finish = () => {
    if (!part) return;
    const p = part;
    part = null;
    if (p.more) {
      for (const [param, hint] of Object.entries(p.more.need || {})) if (p.use[param] === undefined) fail(p.n, `${p.kind} ${p.use.table}: needs ${hint}`);
      screen.body.push(p.use);
      return;
    }
    if (p.kind === 'LIST' && p.use.export && p.use.columns) p.use.exportColumns = p.use.columns.map((c) => ({ key: c.key, label: c.label || c.key }));
    if (p.kind === 'LIST' || p.kind === 'BOARD' || p.kind === 'DETAIL' || p.kind === 'SUMMARY') {
      for (const need of p.needs || []) if (p.use[need] === undefined) fail(p.n, `${p.kind} ${p.use.table}: needs ${{ fields: 'FORM (...)', status: "BY <column> ('a', 'b')", group: 'GROUP BY <column>' }[need]}`);
      screen.body.push(p.use);
    }
  };
  const SIMPLE = /^(TITLE|NOUN|EMPTY|LABEL|NAME|BACK|LIMIT|MONEY|DONUT)\b/i;
  const CLAUSE = '(?:TITLE|NOUN|EMPTY|LABEL|NAME|BACK|LIMIT|MONEY|DONUT|WHERE|ORDER BY|GROUP BY|VALUE|SEARCH|FILTER|TOGGLE|FORM|COLUMNS|CARDS|KPI|META|BY|ON ROW|ACTION|SHOW|PICK|RELATED|QUERY|ITEMS|ITEM|DAYS|DAY|SUB|DATE|TIME|PLACE|COLOR|FRONT|DUE|INTERVAL|MASTERED|QUESTION|CHOICES|ANSWER|EXPLANATION|COUNT|RESULTS|TEXT|MEANING|PLACEHOLDER|FORMAT|RECENT|EXTRA|PARAM|PER|LAST|LINE|EXPORT|ADD|SKIP|PRICE|IMAGE|LEFT|SOON|OUT|OPEN|CALL|DONE|LOOK|EYEBROW|PROGRESS|LESSON|MINUTES|CERTIFICATE|ID|PERIOD|FILE|COMPARE|AGAINST|CHART|NEEDS|LOCKED|UNLOCKLABEL|UNLOCK|LANES|DETAIL|FLAGS|HEAD|CHIPS|JOIN|TIP|TONE|PRIMARY|SECONDARYOPEN|SECONDARY|OVER|PROMPT|BUTTON|NOTE|INPUT|WEB|PAY|CHARGE|PAID|QTY|CONTENT|BODY|HOLDER|PERSON|BADGE|MORE|SET|THINGS|THING|ROLES|ROLE|PARTS|PART|OF|UNIT|TOTALS|SECTION|GROUP|SPACES|BOOKINGS|START|END|CLOSED|CLOSE|STEP|LENGTH|RESOURCES|RESOURCE|VIEW|CONTACT|STATUS|STATES|NUMBER|TO|LINES|TAX|CAPTION|ALBUM|UPLOAD|ME)\\b';
  // Several clauses may share a line: split after a simple clause (TITLE
  // 'x' NOUN 'y'), and at two or more spaces before a clause word, except in
  // the clauses that hold their own WHERE or JSON.
  const OWN = /^(KPI|ACTION|RELATED|DATA|LET|STATE|BLOCKS|PHOTOS|QUERY|PARAM|EXTRA|SECTION|SET)\b/i;
  const split = [];
  // (in a part whose ACTION is a label alone, such as OFFERS, an ACTION line
  // is split like any other: ACTION 'Buy'  CALL buy  PAY orders)
  let within = null;
  for (const ln of lines.slice(1)) {
    const head = (ln.text.match(/^([A-Za-z]+)\b/) || [])[1]?.toUpperCase();
    if (head && PARTS.includes(head) && !MORE[within]?.clauses?.[head]) within = head;
    const labelOnly = head === 'ACTION' && MORE[within]?.clauses?.ACTION?.[1] === 'str';
    const pieces = OWN.test(ln.text) && !labelOnly ? [ln.text] : ln.text.split(new RegExp(`\\s{2,}(?=${CLAUSE})`, 'i'));
    for (let t of pieces) {
      while (SIMPLE.test(t)) {
        const m = t.match(new RegExp(`^([A-Za-z]+\\s+(?:'(?:[^']|'')*'|[A-Za-z0-9_-]+)|MONEY|DONUT|QTY|COUNT)\\s+(${CLAUSE}[\\s\\S]*)$`, 'i'));
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
    // KPI 'Label' = SUM(col) FROM <table> [WHERE ...] [MONEY]: a big number of
    // its own, over any table; KPIs one after the other share a row.
    if (kw === 'KPI' && /\sFROM\s+[A-Za-z_]/i.test(rest)) {
      finish();
      const k = kpiOf(rest, n, true);
      const name = `kpi_${kpiN++}`;
      screen.data[name] = withDays(`SELECT ${k.value} AS v FROM ${k.table}${k.where ? ` WHERE ${k.where}` : ''}`);
      const item = { label: k.label, of: name, format: k.format };
      if (kpiRow && screen.body.at(-1) === kpiRow) kpiRow.kpis.push(item);
      else { kpiRow = { kpis: [item] }; screen.body.push(kpiRow); }
      continue;
    }
    if (PARTS.includes(kw) && !(part && part.more && part.more.clauses[kw])) {
      finish();
      if (kw === 'HEADING') { screen.body.push({ h: literal(rest) }); continue; }
      if (kw === 'PAGE') {
        // PAGE front [LAYOUT editorial]: a front page, and its structure.
        const pm = rest.match(/^([a-z]+)(?:\s+LAYOUT\s+(.+))?$/i);
        if (!pm) fail(n, 'PAGE <kind>, such as PAGE front');
        screen.page = pm[1].toLowerCase();
        if (pm[2] && parseLayout(pm[2])) screen.layout = parseLayout(pm[2]);
        continue;
      }
      // LAYOUT <hero | split | editorial | dashboard | gallery | tool>: the
      // screen's page structure (quality.mjs LAYOUTS). A word the kit does
      // not know is left out rather than failing the build.
      if (kw === 'LAYOUT') { const l = parseLayout(rest.replace(/^'|'$/g, '')); if (l) screen.layout = l; continue; }
      // FOR owner | members | everyone: who the screen is for (engine/audience.mjs).
      if (kw === 'FOR') {
        const a = rest.toLowerCase().replace(/^'|'$/g, '');
        if (!['owner', 'admins', 'members', 'everyone'].includes(a)) fail(n, 'FOR owner | members | everyone');
        audience = a === 'admins' ? 'owner' : a;
        continue;
      }
      if (kw === 'TEXT') { screen.body.push({ text: literal(rest), muted: true }); continue; }
      if (kw === 'DATA') {
        // DATA name AS (SELECT ...) [PARAMS (name = expression, ...)]
        // DATA name = <JavaScript giving the rows, or the SELECT to run> [PARAMS (...)]
        // DATA name JSON <the entry as JSON>
        const js = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+JSON\s+([\s\S]+)$/i);
        if (js) { try { screen.data[js[1]] = JSON.parse(js[2]); } catch (e) { fail(n, `DATA ${js[1]} JSON: ${e.message}`); } continue; }
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(?:\s+AS\s+|=)([\s\S]+)$/i);
        if (!m) fail(n, 'DATA <name> AS SELECT ...');
        const isExpr = !/^\s*[A-Za-z_][A-Za-z0-9_]*\s+AS\s/i.test(rest);
        let body = m[2].trim();
        let params = null;
        const pm = body.match(/\s+PARAMS\s*(\([\s\S]*\))\s*$/i);
        if (pm) {
          const p = parens(pm[1], n, 'PARAMS');
          if (!p.rest) {
            params = {};
            for (const a of splitTop(p.inside)) { const am = a.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/); if (!am) fail(n, `PARAMS: ${JSON.stringify(a)} is not name = expression`); params[am[1]] = `=${am[2].trim()}`; }
            body = body.slice(0, pm.index).trim();
          }
        }
        if (isExpr) { screen.data[m[1]] = params ? { sql: `=${body}`, params } : `=${body}`; continue; }
        if (body.startsWith('(')) { const p = parens(body, n, 'DATA'); if (!p.rest) body = p.inside.trim(); }
        screen.data[m[1]] = params ? { sql: body, params } : body;
        continue;
      }
      if (kw === 'LET' && /^[A-Za-z_][A-Za-z0-9_]*\s+JSON\s/i.test(rest)) {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+JSON\s+([\s\S]+)$/i);
        try { screen.let[m[1]] = JSON.parse(m[2]); } catch (e) { fail(n, `LET ${m[1]} JSON: ${e.message}`); }
        continue;
      }
      if (kw === 'STATE' || kw === 'LET') {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
        if (!m) fail(n, `${kw} <name> = ${kw === 'STATE' ? 'a JSON value' : 'a JavaScript expression'}`);
        if (kw === 'LET') screen.let[m[1]] = `=${m[2].trim()}`;
        else { try { screen.state[m[1]] = JSON.parse(m[2]); } catch { screen.state[m[1]] = literal(m[2]); } }
        continue;
      }
      // PHOTOS [{"photo": "query", "src": "...", "credit": "..."}, ...]: the
      // photos found for the screen's photo queries, in the order they show.
      if (kw === 'PHOTOS') {
        try { photos = JSON.parse(rest); } catch (e) { fail(n, `PHOTOS takes a JSON list of the photos found: ${e.message}`); }
        if (!Array.isArray(photos) || photos.some((x) => !x || typeof x.photo !== 'string' || typeof x.src !== 'string')) fail(n, 'PHOTOS takes a JSON list of {"photo": "query", "src": "url"}');
        continue;
      }
      if (kw === 'BLOCKS') {
        let blocks;
        try { blocks = JSON.parse(rest); } catch (e) { fail(n, `BLOCKS takes a JSON list of screen blocks: ${e.message}`); }
        screen.body.push(...(Array.isArray(blocks) ? blocks : [blocks]));
        continue;
      }
      // A part of no table (HEADER 'Your day') names its words instead.
      if (MORE[kw] && MORE[kw].bare) {
        if (!/^'(?:[^']|'')*'$/.test(rest)) fail(n, `${kw} 'words'`);
        part = { kind: kw, n, use: { use: MORE[kw].use, [MORE[kw].bare]: literal(rest) }, more: MORE[kw], needs: [], kpis: [] };
        continue;
      }
      const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)(?:\s+AS\s+([A-Za-z_][A-Za-z0-9_]*))?\s*$/i);
      if (!m) fail(n, `${kw} <table> [AS <key>]`);
      const use = { use: MORE[kw] ? MORE[kw].use : { LIST: 'crud', BOARD: 'board', DETAIL: 'detail', SUMMARY: 'summary' }[kw], table: m[1] };
      if (m[2]) use.key = m[2];
      part = { kind: kw, n, use, more: MORE[kw] || null, needs: { LIST: ['fields'], BOARD: ['status'], DETAIL: ['fields'], SUMMARY: ['group'] }[kw], kpis: [] };
      continue;
    }
    if (part && part.more) { moreClause(part, kw, rest, l, n); continue; }
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
      case 'EXPORT': if (part.kind !== 'LIST') fail(n, 'EXPORT belongs to a LIST'); u.export = rest.trim() ? literal(rest) : 'Export CSV'; break;
      case 'FILTER': {
        const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+(?:IN\s+)?([\s\S]+)$/i);
        if (!m) fail(n, "FILTER <column> IN ('a', 'b')");
        let options = splitTop(parens(m[2], n, 'FILTER').inside).map(literal);
        // a yes/no column's chips say what it means ("Paid", "Not paid"), never true and false
        if (options.length && options.every((o) => typeof o === 'boolean')) {
          const name = (u.toggle === m[1] && u.toggleLabel) || m[1].replace(/_/g, ' ').replace(/^is /, '').replace(/^./, (c) => c.toUpperCase());
          options = options.map((o) => ({ value: String(o), label: o ? name : `Not ${name.toLowerCase()}` }));
        }
        u.filter = { column: m[1], options };
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
        const k = kpiOf(rest, n);
        const name = `${key}_k${kpiN++}`;
        const conds = [u.where, k.where].filter(Boolean).map((c) => `(${c})`).join(' AND ');
        screen.data[name] = withDays(`SELECT ${k.value} AS v FROM ${u.table}${conds ? ` WHERE ${conds}` : ''}`);
        u.kpis = [...(u.kpis || []), { label: k.label, of: name, format: k.format }];
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
        let setsOf = null;
        if (m[3]) doIt = `=row => app.open(${jsStr(m[3])}, {id: row.id})`;
        else if (m[4]) {
          const sets = splitTop(m[4]).map((x) => { const mm = x.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/); if (!mm) fail(n, `ACTION SET: ${JSON.stringify(x)} is not col = value`); return [mm[1], literal(mm[2])]; });
          const vals = Object.fromEntries(sets.map(([k, v]) => [k, v === 'TODAY' || v === 'today' ? '@today' : v]));
          doIt = `=row => app.update(${jsStr(u.table)}, row.id, ${JSON.stringify(vals).replace(/"@today"/g, 'app.today()')})`;
          // "sets": the kit leaves the action off a row that has these values already
          setsOf = vals;
        } else { u.deleteLabel = literal(m[1]); break; } // every list has a delete button: this names it
        u.actions = [...(u.actions || []), { label: literal(m[1]), class: 'small ghost', do: doIt, ...(setsOf ? { sets: setsOf } : {}) }];
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
      case 'COUNT': if (part.kind !== 'SUMMARY') fail(n, `${part.kind} does not take COUNT`); u.count = literal(rest); break;
      default: fail(n, `${part.kind} does not take ${JSON.stringify(l.slice(0, 30))}`);
    }
  }
  finish();
  // A field FROM a table picks among its rows: their query is the screen's data.
  const picks = (o) => {
    if (Array.isArray(o)) { o.forEach(picks); return; }
    if (!o || typeof o !== 'object') return;
    for (const f of Array.isArray(o.fields) ? o.fields : []) {
      if (!f || !f.from) continue;
      const { table, value, label } = f.from;
      const name = `${table}_${label === value ? value : `${value}_${label}`}_choices`;
      screen.data[name] = `SELECT ${value} AS value, ${label} AS label FROM ${table} ORDER BY ${label} LIMIT 500`;
      f.options = `=[...new Map(${name}.filter((r) => r.value != null && r.value !== '').map((r) => [String(r.value), {value: r.value, label: String(r.label ?? r.value)}])).values()]`;
      delete f.from;
    }
    for (const v of Object.values(o)) if (v && typeof v === 'object') picks(v);
  };
  picks(screen.body);
  for (const k of ['state', 'data', 'let']) if (!Object.keys(screen[k]).length) delete screen[k];
  return { slug: head[1], title, screen, ...(photos?.length ? { photos } : {}), ...(audience ? { for: audience } : {}) };
}

// A clause of one of the newer parts: sets one parameter of its pattern.
function moreClause(part, kw, rest, l, n) {
  const c = part.more.clauses[kw];
  // MONEY alone in a part that takes FORMAT is FORMAT money, as a KPI or a column says it
  if (!c && kw === 'MONEY' && !String(rest || '').trim() && part.more.clauses.FORMAT) { part.use[part.more.clauses.FORMAT[0]] = 'money'; return; }
  if (!c) fail(n, `${part.kind} takes ${Object.keys(part.more.clauses).join(', ')}, not ${JSON.stringify(l.slice(0, 30))}`);
  const [param, kind] = c;
  const u = part.use;
  switch (kind) {
    case 'name': if (!NAME.test(rest)) fail(n, `${kw} <column>`); u[param] = rest; break;
    case 'slug': if (!/^[a-z][a-z0-9-]*$/.test(rest)) fail(n, `${kw} <a view's or function's slug>`); u[param] = rest; break;
    case 'day': if (!NAME.test(rest)) fail(n, `${kw} <column holding 'YYYY-MM-DD'>`); u[param] = rest; u.day = true; break;
    case 'names': u[param] = names(rest, n, kw); break;
    case 'list': u[param] = names(rest.replace(/^\(\s*([\s\S]*?)\s*\)$/, '$1'), n, kw); break;
    // words in quotes, or an =expression of the screen's data
    case 'text': if (/^=/.test(rest)) u[param] = rest; else u[param] = literal(rest); break;
    // 'Label' [SET col = value | CALL slug | OPEN slug]
    case 'action': {
      const am = rest.match(/^('(?:[^']|'')*')\s*(?:(SET)\s+(\w+\s*=\s*.+)|(CALL|OPEN)\s+([a-z][a-z0-9-]*))?\s*$/i);
      if (!am) fail(n, `${kw} 'Label' [SET column = value | CALL <function> | OPEN <view>]`);
      u[param] = literal(am[1]);
      if (am[2]) u.set = am[3].trim();
      if (am[4]) u[am[4].toLowerCase()] = am[5];
      break;
    }
    case 'str': u[param] = literal(rest); break;
    case 'word':
      u[param] = rest.toLowerCase();
      if (kw === 'PER' && !['day', 'week', 'month'].includes(u[param])) fail(n, 'PER day | week | month');
      break;
    case 'flag': if (rest) fail(n, `${kw} takes nothing after it`); u[param] = true; break;
    case 'num': if (!/^\d+$/.test(rest)) fail(n, `${kw} <a whole number>`); u[param] = Number(rest); break;
    // a query in parentheses, as a clause holds one, is the query
    case 'sql': case 'js': if (!rest) fail(n, `${kw} needs a value`); u[param] = kind === 'sql' && /^\(\s*SELECT\b[\s\S]*\)$/i.test(rest) ? rest.slice(1, -1).trim() : rest; break;
    case 'fields': u[param] = splitTop(parens(rest, n, kw).inside).map((f) => fieldSpec(f, n)); break;
    case 'cols': u[param] = splitTop(parens(rest, n, kw).inside).map((x) => columnSpec(x, n)); break;
    // (0, 6): a list of whole numbers
    case 'nums': {
      const inside = rest.trim().startsWith('(') ? parens(rest, n, kw).inside : rest;
      const list = splitTop(inside).map((x) => x.trim()).filter(Boolean);
      if (list.some((x) => !/^\d+$/.test(x))) fail(n, `${kw} (whole numbers, such as 0, 6)`);
      u[param] = list.map(Number);
      break;
    }
    // ('words', 'words'): a list of words
    case 'strs': {
      const inside = rest.trim().startsWith('(') ? parens(rest, n, kw).inside : rest;
      u[param] = splitTop(inside).map((x) => (/^'/.test(x.trim()) ? literal(x.trim()) : x.trim())).filter(Boolean);
      break;
    }
    // SECTION 'Title' (SELECT ...) [COLUMNS (...)] [TOTAL (col, ...)] [NOTE 'words']: a section of a report, one per line
    case 'section': {
      const tm = rest.match(/^('(?:[^']|'')*')\s*/);
      if (!tm) fail(n, `SECTION 'Title' (SELECT ...) [COLUMNS (...)] [TOTAL (col, ...)] [NOTE 'words']`);
      const q = parens(rest.slice(tm[0].length), n, 'SECTION');
      const sec = { title: literal(tm[1]), query: q.inside.trim() };
      let more = q.rest;
      while (more) {
        let mm;
        if ((mm = more.match(/^COLUMNS\s*/i))) { const p = parens(more.slice(mm[0].length), n, 'COLUMNS'); sec.columns = splitTop(p.inside).map((x) => columnSpec(x, n)); more = p.rest; continue; }
        if ((mm = more.match(/^TOTAL\s*/i))) { const p = parens(more.slice(mm[0].length), n, 'TOTAL'); sec.total = names(p.inside, n, 'TOTAL'); more = p.rest; continue; }
        if ((mm = more.match(/^NOTE\s+('(?:[^']|'')*')\s*/i))) { sec.note = literal(mm[1]); more = more.slice(mm[0].length); continue; }
        fail(n, `SECTION: cannot read ${JSON.stringify(more.slice(0, 40))}; after the query come COLUMNS (...), TOTAL (col, ...) and NOTE 'words'`);
      }
      u[param] = [...(u[param] || []), sec];
      break;
    }
    case 'param': {
      const m = rest.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
      if (!m) fail(n, 'PARAM <name> = <JavaScript expression> (its value for :name in WHERE)');
      u[param] = { ...(u[param] || {}), [m[1]]: `=${m[2].trim()}` };
      break;
    }
    default: fail(n, `${kw}: unknown`);
  }
}

// ---- a screen (JSON) as a statement

// Two screens are the same when their JSON is, empty parts aside.
const canon = (v) => (Array.isArray(v) ? v.map(canon) : v && typeof v === 'object'
  ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, canon(v[k])])) : v);
export const sameScreen = (a, b) => {
  const norm = (x) => { const o = { ...(x || {}) }; for (const k of ['state', 'data', 'let']) if (o[k] && !Object.keys(o[k]).length) delete o[k]; if (!o.body) o.body = []; return JSON.stringify(canon(o)); };
  return norm(a) === norm(b);
};

const oneLine = (s) => typeof s === 'string' && !/[\n\r]/.test(s) && !/\s{2,}/.test(s);
const sqlText = (s) => `'${String(s).replace(/'/g, "''")}'`;

// A screen as a CREATE SCREEN statement that compiles back into exactly that
// screen: its state, data and lets as STATE, DATA and LET lines (SQL as SQL),
// headings as HEADING, the rest of its blocks as BLOCKS. With plain, data
// and lets that are not SQL or one-line expressions are given as JSON.
// Returns null when no statement compiles back into it.
export function screenStatement(slug, title, screen, { plain = false } = {}) {
  if (!screen || typeof screen !== 'object' || Array.isArray(screen)) return null;
  if (Object.keys(screen).some((k) => !['state', 'data', 'let', 'body', 'page', 'layout'].includes(k))) return null;
  const NAME_OK = /^[A-Za-z_][A-Za-z0-9_]*$/;
  const L = [`CREATE SCREEN ${slug}${title ? ` TITLE ${sqlText(title)}` : ''}`];
  if (screen.page !== undefined) { if (typeof screen.page !== 'string' || !/^[a-z]+$/.test(screen.page)) return null; L.push(`PAGE ${screen.page}`); }
  if (screen.layout !== undefined) { if (parseLayout(screen.layout) !== screen.layout) return null; L.push(`LAYOUT ${screen.layout}`); }
  for (const [k, v] of Object.entries(screen.state || {})) { if (!NAME_OK.test(k)) return null; L.push(`STATE ${k} = ${JSON.stringify(v)}`); }
  for (const [k, v] of Object.entries(screen.data || {})) {
    if (!NAME_OK.test(k)) return null;
    const params = (p) => (p && typeof p === 'object' && !Array.isArray(p) && Object.keys(p).length && Object.entries(p).every(([pk, pv]) => NAME_OK.test(pk) && typeof pv === 'string' && pv.startsWith('=') && oneLine(pv))
      ? ` PARAMS (${Object.entries(p).map(([pk, pv]) => `${pk} = ${pv.slice(1)}`).join(', ')})` : null);
    if (!plain && typeof v === 'string' && !v.startsWith('=') && /^\s*(select|with)\b/i.test(v)) L.push(`DATA ${k} AS (${v})`);
    else if (!plain && typeof v === 'string' && v.startsWith('=') && oneLine(v)) L.push(`DATA ${k} = ${v.slice(1)}`);
    else if (!plain && v && typeof v === 'object' && Object.keys(v).length === 2 && typeof v.sql === 'string' && !v.sql.startsWith('=') && /^\s*(select|with)\b/i.test(v.sql) && params(v.params)) L.push(`DATA ${k} AS (${v.sql})${params(v.params)}`);
    else if (!plain && v && typeof v === 'object' && Object.keys(v).length === 2 && typeof v.sql === 'string' && v.sql.startsWith('=') && oneLine(v.sql) && params(v.params)) L.push(`DATA ${k} = ${v.sql.slice(1)}${params(v.params)}`);
    else L.push(`DATA ${k} JSON ${JSON.stringify(v)}`);
  }
  for (const [k, v] of Object.entries(screen.let || {})) {
    if (!NAME_OK.test(k)) return null;
    L.push(!plain && typeof v === 'string' && v.startsWith('=') && oneLine(v) ? `LET ${k} = ${v.slice(1)}` : `LET ${k} JSON ${JSON.stringify(v)}`);
  }
  let run = [];
  const flush = () => { if (run.length) L.push(`BLOCKS ${JSON.stringify(run, null, 2)}`); run = []; };
  for (const b of Array.isArray(screen.body) ? screen.body : []) {
    const keys = b && typeof b === 'object' && !Array.isArray(b) ? Object.keys(b).sort().join(',') : '';
    if (!plain && keys === 'h' && oneLine(b.h)) { flush(); L.push(`HEADING ${sqlText(b.h)}`); }
    else if (!plain && keys === 'muted,text' && b.muted === true && oneLine(b.text)) { flush(); L.push(`TEXT ${sqlText(b.text)}`); }
    else run.push(b);
  }
  flush();
  const text = L.join('\n');
  try { if (sameScreen(compileStatement(text).screen, screen)) return text; } catch { /* not this way */ }
  return plain ? null : screenStatement(slug, title, screen, { plain: true });
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
      v.statement = code[v.slug].trim(); // the screen's source, kept with it
      // its TITLE is the screen's title: the source the request changed
      if (out.title) v.title = out.title;
      if (out.for && v.for === undefined) v.for = out.for;
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
    a choice among another table's rows: col 'Label' FROM <table> (<value col>, <label col>)  -- client FROM clients (id, name) keeps the id; category FROM categories (name) keeps the name. Use it whenever the choices are rows people add, never a fixed list
    a person who signed up to the app: col 'Label' PERSON keeps their account's id (who a job is for when they sign in); the owner picks a name, never types an id
    several of them: col 'Label' PEOPLE on a keyword[] column keeps their ids (a renovation's crew), the column an access WHERE col = :user_id reads
  COLUMNS (col 'Label' DATE, col 'Label' MONEY BADGE, col 'Label' MONEY = <JavaScript of row>, ...)   after the label, one format word: ${COLUMN_FORMATS.join(' ')} (DAYS reads a day as "in 3 days", DURATION minutes as "1 h 05", CLOCK seconds as "5:30", HOURS a number as "2.5 h", PCT a percent as "12.5%"), and BADGE
  CARDS <title col> [SUB col, col]      (cards instead of a table)
  KPI 'Label' = COUNT(*) | SUM(col) | AVG(col) [WHERE condition] [${KPI_FORMATS.join(' | ')}]   (one line each; SQL over the table)
  ON ROW OPEN <view slug>   ACTION 'Label' OPEN <view slug> | SET col = value, ... | DELETE ('Label' names the delete button every list has)
  EXPORT ['Download CSV']               (a button that downloads the rows shown, searched and filtered, as a CSV file with the COLUMNS; no function needed)
BOARD <table> [AS key]         -- the board pattern
  BY <status col> ('new', 'doing', 'done')  NAME <title col>  META col, col  FORM (...)  TITLE  NOUN  WHERE  ORDER BY
DETAIL <table> [AS key]        -- the detail pattern: one row (opened with app.open(slug, {id})), edit, related rows
  NAME <heading col>  FORM (...)  SHOW (columns)  PICK (columns of the pick list)  BACK <view slug>  TITLE  NOUN
  RELATED <table> BY <its column holding the id> [TITLE 'Title'] [COLUMNS (...)] [ORDER BY ...]   (one line each)
SUMMARY <table> [AS key]       -- the summary pattern: a total and a bar (or donut) per group, together under its heading
  GROUP BY <SQL>  VALUE SUM(col)  LABEL 'Spent'  WHERE <SQL>  MONEY  DONUT  LIMIT 20  TITLE 'Heading'  COUNT 'Customers' (the number of groups beside the total, only when it means something)
CHECKS <table of checks> [AS key] -- the daily check-off, for the screen people open every day: each thing with one big tap for today, its last days, its streak, and how many are done today (habits, medication, plants watered, chores); a check is a row
  ITEMS <table of the things>  ITEM <checks column holding the thing's id>  DAY <checks column, 'YYYY-MM-DD'>  NAME <things column>  DAYS 7  WHERE <SQL on the things>  ORDER BY <SQL>  ADD  NOUN 'habit'  TITLE 'Today'  SKIP <bool checks column: a day skipped keeps the streak>
WEEK <table of marks> [AS key] -- the week pattern: a row per thing with its 7 days to tick (habits, chores); a mark is a row
  ITEMS <table of the things>  ITEM <marks column holding the thing's id>  DAY <marks column, 'YYYY-MM-DD'>  NAME <things column>  SUB <things column>  LABEL 'Done'  EXTRA {points: it.points}  TITLE
CALENDAR <table> [AS key]      -- the calendar pattern: a month, the picked day's rows, a form to add one
  DATE <'YYYY-MM-DD' column>  NAME <col>  TIME <col>  PLACE <col>  COLOR <col>  FORM (...)  WHERE  TITLE  NOUN
FLASHCARDS <table> [AS key]    -- spaced repetition: a card, its answer, then Again/Hard/Good/Easy; due and new/learning/mastered counts
  FRONT <col>  BACK <col>  DUE <keyword col>  INTERVAL <int col>  MASTERED 21  WHERE deck = :deck  PARAM deck = state.deck  TITLE
QUIZ <table> [AS key]          -- multiple choice, one question after another, with a score (saved per try with RESULTS)
  QUESTION <col>  CHOICES <col: a list, or one choice per line>  ANSWER <col: the right choice or its index>  EXPLANATION <col>  COUNT 10  RESULTS <table>  EXTRA {deck: state.deck}  WHERE  PARAM  TITLE
FIND <table> [AS key]          -- the search pattern: words and meaning ranked together, the picked row, rows like it
  TEXT <text col>  MEANING <meaning col>  NAME <col>  SUB col, col  SHOW (columns)  LIMIT 20  PLACEHOLDER 'Search notes'  WHERE  PARAM  TITLE
DASHBOARD <table> [AS key]     -- a value over the last N days against the N before: per day, by a group, newest rows
  DATE <timestamp col> | DAY <'YYYY-MM-DD' col>  VALUE SUM(col)  LABEL 'Hours'  FORMAT number | money | hours  DAYS 30  BY <group col>  RECENT (columns)  WHERE  TITLE
TREND <table> [AS key]         -- a value per day, week or month over the last periods, as bars (LINE for a line), with the total and the average
  DATE <timestamp or 'YYYY-MM-DD' col>  VALUE SUM(col)  PER day | week | month  LAST 12  LABEL 'Distance (km)'  FORMAT number | money | hours  WHERE  LINE  TITLE
NEXT <table> [AS key]          -- the next thing to come as the headline (a date block, its title, when and where, how long until it, its main action), the ones after it as dated cards: classes, matches, shows, meetings, open days. Put it first on a page whose visitors come for what is next (above any slogan), on a front page too
  DATE <day col>  NAME <col>  TIME <col>  PLACE <col>  TEXT <col>  BADGE <col>  WHERE <SQL>  EYEBROW 'Next match'  ACTION 'Get tickets'  OPEN <view, gets {id}> | CALL <function>  MORE 6  TITLE 'Coming up'  EMPTY
DUE <table> [AS key]           -- what falls due, overdue first, each with its state as a coloured badge (Overdue, Due soon, Later), how far off it is, its day and the columns that say where or what it is, the counts first: expiry dates, renewals, licences, inspections, deadlines. Choose it whenever the request asks what is expiring, ending, overdue or due soon
  DUE <day col>  NAME <col>  SUB col, col  SOON 30  WHERE <the rows still to watch>  NOUN 'permit'  LABEL 'Permit'  OPEN <view>  ACTION 'Mark renewed'  SET {col: expression of r and today} | CALL <function>  TITLE  EMPTY
LEND <loans table> [AS key]    -- things lent out and back (tools, keys, cameras, kit, vehicles): each thing In, Out with whom until when, or Overdue; check out with a short form, check in with a tap, the latest loans. A loan is a row: the thing, who has it, the day out, the day due, the day back (empty while out)
  ITEMS <table of the things>  ITEM <loans col with the thing's id>  NAME <things col>  SUB <things col>  HOLDER <loans col>  PERSON (holder is a person who signed up)  OUT <day out col>  DUE <due col>  BACK <day back col>  DAYS 14  NOUN 'tool'  LABEL 'Tool'  TITLE
PLAN <plan table> [AS key]     -- a week to plan: each named day (Monday 16 Nov) with what is picked for it from ITEMS, Change and Clear, a Pick menu, arrows to other weeks; with PARTS, what the week adds up to beside it (summed over the picked things' parts: materials to buy, hours, parts). Choose it over a table of dated rows whenever the request plans a week
  ITEMS <table of the things to pick>  DAY <plan day col>  ITEM <plan col with the thing's id>  NAME <things col>  PARTS <table of what each thing needs>  OF <parts col with the thing's id>  PART <its name col>  QTY <col>  UNIT <col>  TITLE 'This week'  PICK 'Pick a job'  TOTALS 'Materials to buy'
REPORT 'Month-end report'      -- a report in numbered sections, each its own query drawn as a table with a total row and a note under it: a board or month-end report, any request that names a report's parts or asks it to read like a report (1 Visits by month, 2 Open tickets by age). One SECTION per line, in order, up to 8
  SUB 'the period it covers, or =expression'  SECTION 'Title' (SELECT ... :today is the day) COLUMNS (col 'Label' MONEY, ...) TOTAL (col, col) NOTE 'what it shows, why a value is missing'
TEAM <link table> [AS key]     -- who is on what: the app's people put on things (accounts, sites, classes) with a role, each thing with its people, things nobody is on, put someone on (picked by name) or take them off. The page an access rule WHERE <col> IN (SELECT <thing col> FROM <link table> WHERE <person col> = :user_id) reads; give it FOR owner
  THINGS <table of the things>  PERSON <link col with the person's id>  THING <link col with the thing's id>  NAME <things col>  ROLE <link col>  ROLES ('lead', 'member')  LABEL 'Account'  TITLE
PRICES <table> [AS key]        -- prices that read at a glance: each name and its price on one line with a dotted leader, a note under it, grouped under headings (services, fees, plans); with COLUMNS a rate table with labelled columns (one card per row on a phone). Choose it whenever prices are listed
  NAME <col>  PRICE <col: a number or words such as 'from 40'>  NOTE <col>  GROUP <col of the heading>  BADGE <col>  UNIT 'per hour'  COLUMNS (col 'Label' MONEY, ...)  WHERE  ORDER BY  OPEN <view, gets {id}>  TITLE  EYEBROW  EMPTY
TIMETABLE <table> [AS key]     -- a weekly timetable: rows that repeat each week on a weekday at a time (classes, rehearsals, practices, sessions), each day a table of them in time order with who leads and where; with BOOKINGS, a badge of the places left on that date (Only 2 left, Full) and a Book button on each row with places (it adds a booking to BOOKINGS, or OPEN or CALL takes it); Previous and Next week buttons. Choose it whenever the request lists weekly classes or a schedule people book into
  DAY <weekday col: Monday, mon or 0-6>  TIME <'HH:MM' col>  MINUTES <col>  NAME <col>  SUB <col: who leads>  PLACE <col>  SPACES <capacity col>  BOOKINGS <table of places taken>  OF <its col with the row's id>  DATE <its 'YYYY-MM-DD' col>  ACTION 'Book'  OPEN <view, gets {id, date}> | CALL <function>  TITLE
SLOTS <bookings table> [AS key] -- book a time: a strip of days, the chosen day's free times between opening and closing, taken ones greyed out, so nothing is booked twice; with RESOURCE the person first picks whom or what (a staff member, a court). A chosen time opens the view that finishes it or calls the rule that takes it (the rule refuses a clash: REQUIRE (SELECT COUNT(*) FROM <bookings> WHERE <resource col> = :resource AND times_overlap(<start col>, <end col>, :start, :end)) = 0 ELSE 'That time was just taken')
  START <'YYYY-MM-DDTHH:MM' col>  END <col>  OPEN '09:00'  CLOSE '17:00'  STEP 30  LENGTH 45  CLOSED (0, 6)  DAYS 14  RESOURCE <col>  RESOURCES <table>  LABEL <its name col>  VIEW <view, gets {start, end, resource}> | CALL <function>  TITLE  NOTE
INBOX <table> [AS key]         -- what people send from a public form (enquiries, callback or quote requests, reports, applications), worked through by the owner: new ones counted first, each with who, how to reach them, what they asked and when, and one tap to move it on (Start, Done, Reopen). Give it FOR owner, beside the public form that fills the table (its status column starts empty or as the first state)
  NAME <col>  CONTACT col, col  TEXT <col>  STATUS <col>  STATES ('new', 'in progress', 'done')  SUB col, col  NOUN 'request'  TITLE
STANDINGS <table> [AS key]     -- standings worked out from rows: each one's score (points, wins, a total) ranked, ties sharing a place, medals for the first three, a second figure under each name, the viewer highlighted; a league table, a challenge, a contest. QUERY when a row scores for two at once (a match's two sides: a subquery of SELECT ... UNION ALL SELECT ... grouped)
  BY <col of who scores>  VALUE SUM(points)  SUB COUNT(*)  UNIT 'game'  WHERE  QUERY (SELECT name, value[, sub] ...)  ME <expression: the viewer's name>  FORMAT number | money  LIMIT 10  TITLE
DOCUMENT <table> [AS key]      -- one record as the document a person keeps or sends (invoice, receipt, quote, statement, certificate): from and for whom, its number and date, its lines with the subtotal, tax and total, a note; opened with {id} from a list, else the latest. Choose it whenever the request says invoice, receipt, quote or certificate
  LABEL 'Invoice'  NUMBER <col>  DATE <col>  TO <col: whom it is for>  BY 'who it is from, or =expression'  LINES <table of its lines>  OF <their col with the record's id>  ITEM <col>  QTY <col>  PRICE <col>  TAX 20  TOTAL <col, when it has no lines>  NOTE <col>  BACK <view>
GALLERY <table> [AS key]       -- the owner's own photos (a table of stored files with a caption and an album) as a gallery that opens large, albums as chips; with UPLOAD (on the owner's screen) photos dropped in become rows. Use it whenever the request says the owner's photos, gallery or portfolio: never stock photos in their place
  FILE <col with the stored file's id>  CAPTION <col>  ALBUM <col>  WHERE  UPLOAD  LOOK mosaic | masonry | grid  TITLE  EYEBROW  EMPTY
VARIANCE <table> [AS key]      -- a value compared period by period (a total month over month, quarter over quarter): a row per period in time order with the value, its change and its change in percent (signed, coloured, with bars), the biggest increase and decrease as big numbers, and a CSV download; any request to compare periods (variance, month over month, growth) is this part. It brings its own big numbers (the total, the biggest increase and decrease) and a note on how change is worked out: give no KPI of your own beside it
  PERIOD <col or SQL>  VALUE <col> | SUM(col) GROUP BY <same as PERIOD>  COMPARE <a second value per period, drawn beside it>  AGAINST '<what it is>'  CHART bars | none  ORDER BY <oldest first: a date sorts as it is, a month name needs CASE month WHEN 'Jan' THEN 1 ... END>  LABEL '<what the value is>'  NAME 'Month'  FORMAT number | money  LAST 24  WHERE  FILE '<name>.csv'  TITLE
OFFERS <table> [AS key]        -- what people order or book while some are left (products with stock, workshops with seats, tours with places): a card per row with its photo, date and price, 'Only 2 left' when few remain, the OUT word (its card in the "out" state) and no action when none; the action opens the view that finishes the order or booking, or calls a rule that checks and lowers the count in one step (its REQUIRE refuses the last one twice). A shelf of products, a list with prices, the upcoming sessions or classes are this part, never a LIST with a form a visitor could edit, and on a front page it comes right after the hero, where visitors see it first
  LEFT <count col>  NAME <col>  TEXT <col>  PRICE <col>  IMAGE <col>  DATE <col>  WHERE day >= :today  ORDER BY day  SOON 3  OUT 'Full'  ACTION 'Book a seat'  OPEN <view slug, gets {id}> | CALL <function slug, gets {id}, returns the new row's id>  PAY <table of that row>  CHARGE <its amount column>  PAID <its status column>  QTY (a person takes several: a quantity beside the action, 1 up to what is left; the call gets {id, qty}, its rule REQUIREs the count >= :qty, lowers it by :qty and charges :qty times the price)  COUNT (every card says how many are left, not only when few)  DONE 'Booked'  LOOK cards | slides  TITLE  EYEBROW  ID <anchor for a nav link>
  An app asked to sell or take bookings always takes them: never tell visitors that checkout is off or that they can only browse. A visitor orders or books without an account unless the request asks for one: the rule that takes it reads no :user_id (the form asks a name and an email when the owner needs them) and says ALLOW CALL ON <rule> TO everyone. Where one person may take more than one (products, places on a workshop), the one-tap order gives QTY and its rule works with :qty. The order is a row of <table> (its rule REQUIREs the count and lowers it; its refusal is said under the button that asked); when the request asks for card payment, OFFERS takes it in one tap (CALL a rule that adds the row and RETURNs its id, then PAY <its table> CHARGE <its amount column> PAID <its status column>), or a confirmation screen calls app.pay({table: '<table>', id: <the new row's id>, amount: '<its amount column>', status: '<its status column>'}) and offers "Pay by card" opening the url it returns (a hosted page: never card fields in the app); otherwise it says how payment happens
HEADER 'Your day'              -- a screen's header: its title, one plain line of what matters now, and its actions on the right (the main one filled). A person's own small app says its few figures in this line ('12 planned this week, 3 to act on') rather than in big number cards
  SUB '=expression of the screen's data (counts with app.fmt.count), or words'  PRIMARY 'Add a session'  OPEN <view> | CALL <function>  SECONDARY '=\'Archived (\' + n + \')\''  SECONDARYOPEN <view>
ASK 'A read of your month'     -- the model reads rows and answers a standing question about them in a panel (the answer kept, Ask again, a plain note when the app's model is off). Put it on the first screen beside the rows it reads when the request asks for the model's judgement
  OVER (SELECT only the columns the question needs FROM ...)  PROMPT 'the question in full: what to answer, how long, in what shape'  BUTTON 'Read my month'  INPUT 'Anything it should know (optional)'  PLACEHOLDER 'Goals, plans, limits'  NOTE 'a line under the title'  WEB (it may search the web)
LANES <table> [AS key]         -- rows as cards in lanes along a rail, one lane per value of a column the person thinks in (time of day, stage, day, room): its name, a short detail in the mono face, a muted line, badges pointing at related rows, and its action. Choose it over a table when the rows have such a grouping; keep a table for comparing many columns
  BY <lane column>  LANES 'Morning:With breakfast, Evening:With dinner' (the lanes' order, a note after a colon)  NAME <col>  DETAIL <col>  SUB <col>  FLAGS (SELECT <the card's id> AS row, <words> AS label [, 'warn' | 'bad' | 'ok' | 'info' AS tone] FROM ... joined to the related rows; the words say what the relation is, by the same CASE as the tone: a good one never reads as a problem)  WHERE  ORDER BY  ACTION 'Stop' SET <col> = <value> | CALL <function> | OPEN <view>  EMPTY 'words'  TITLE
CALLOUTS <table> [AS key]      -- rows the person should act on as callout cards (clashes, risks, warnings, findings, advice): a severity label, a title, the things it is about as chips, the explanation and a highlighted tip. Choose it over a table whenever each row asks for an action
  HEAD <title col>  CHIPS (col, col)  JOIN '↔'  TEXT <col>  TIP <col>  TONE <SQL giving 'bad' | 'warn' | 'ok' | 'info', a CASE on its severity>  LABEL <SQL giving the severity words>  WHERE  ORDER BY (most pressing first)  SUB 'a line under the title'  EMPTY  TITLE
STEP <lessons table> [AS key]  -- the page of ONE lesson (or module, chapter, step of a path), the view LESSONS opens with {id}: its number, title and text in full, its questions answered one by one with right or wrong and the explanation, then one main action that marks it done (a row of PROGRESS, once) and opens the next one, or BACK after the last. A wrong answer never locks the way on. Every course's lesson view is this part, never JavaScript
  PROGRESS <table of ones done>  ITEM <its column holding the lesson's id>  CONTENT <rule or function: {id} -> {body, questions: [{question, choices, answer, explanation}]}, refusing who may not have it>  BODY <lessons column with the text, when it is open to all>  NAME <title col>  ORDER BY position  BACK <the course view>  NOUN 'lesson'  DONE 'Mark as done'
LESSONS <lessons table> [AS key] -- a course's lessons with each learner's progress: a bar of how many are done, the next lesson as the main action ('Start lesson 3'), Done on each one done, the certificate once all are; a lesson done is a row of PROGRESS (declare ALLOW ALL ON <progress> TO members WHERE <person col> = :user_id); a paid course gives NEEDS: until it holds the lessons stay shut with the LOCKED words and the UNLOCK button, and each lesson's own content comes through a rule that REQUIREs the same
  PROGRESS <table of lessons done>  NEEDS (SELECT ... a row when the person may open them: a paid enrolment)  LOCKED 'what they are told until then'  UNLOCK <the view that enrols or takes payment>  UNLOCKLABEL 'Enrol'  LESSON <its column holding the lesson's id>  NAME <col>  TEXT <col>  MINUTES <col>  ORDER BY position  WHERE  PARAM  OPEN <lesson view slug, gets {id}>  CERTIFICATE <view slug>  NOUN 'lesson'  TITLE
KPI 'Label' = SUM(col) | COUNT(*) | AVG(col) FROM <table> [WHERE condition] [${KPI_FORMATS.join(' | ')}]   -- a big number over any table, outside a part (KPIs one after the other share a row); :today works in WHERE: this month is WHERE date_trunc('month', day) = date_trunc('month', :today), the last 30 days WHERE days_between(day, :today) <= 30
FOR owner | members           -- who the screen is for, a line of its own: owner for managing what a public page shows (visitors of the published app never get it in the menu), members for signed-in people; leave it out for everyone
LAYOUT <${LAYOUT_NAMES.join(' | ')}>   -- the screen's page structure, its second line (PAGE front, a site's front page, takes one too): ${Object.entries(LAYOUT_SHORT).map(([k, v]) => `${k}: ${v}`).join('; ')}. Give one in EVERY statement, not only the first: the first screen's from the art direction's layout, each other screen's by what it is for (numbers and charts: dashboard; one thing to do, a check-in, a quiz: tool; one row, lessons, a menu: editorial; products or photos: gallery; a list beside its form: split); screens of one app need not share one. A screen that gives none gets the one that fits its parts
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

Say what you can with the parts and their words, and keep JavaScript for what they cannot say: a thing done each day is CHECKS (never a grid of days in BLOCKS or code), a total is a KPI (with FROM over another table), a value per week is a TREND, periods compared with their change and change in percent are a VARIANCE, an amount reads with MONEY and a day with DATE or DAYS, a computed column is col 'Label' MONEY = row.a - row.b, and arithmetic over rows is SQL (the SQL functions, a QUERY with joins) rather than app.fmt, lib calls or reduce() in LET and BLOCKS.

A dashboard (a request to see how things are going) leads with its headline figure and its chart, not a wall of number cards and a table: at most one row of KPIs, charts before tables, and the breakdowns chosen from the data's own dimensions (its groups, its periods, what the request names).

Show the figures the request asks for and the ones its job needs, not every figure the data allows: a big number nobody asked for crowds out the ones that were.

A report the request asks for (to read or hand on) is LAYOUT report: a HEADING naming it, then numbered sections, one per thing the request names, in its order, each starting with its own heading (a part's TITLE).

A screen that is mostly custom (a game, a chart of its own) may be a CREATE SCREEN with a BLOCKS part, or a JSON "screen" in the plan as described above.

The statement is the screen's source: the app keeps it and shows it to you (format="statement"). To change such a screen, give "edits" in its plan entry, [{"old": "text copied exactly from the statement, occurring once", "new": "its replacement"}], or a new statement in its code block when most of it changes. A screen shown as JSON (format="screen") takes a "patch" as described above.`;

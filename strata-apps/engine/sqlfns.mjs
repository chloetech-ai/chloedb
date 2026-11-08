// SQL functions the library keeps in Strata (library/sql/*.sql): arithmetic
// that helpers did in JavaScript (money, days and deadlines, stock, grades,
// training numbers...) written as Strata stored functions, so a screen's
// query computes it where the rows are, and an app holds no code for it.
//
//   CREATE OR REPLACE FUNCTION due_state(due TEXT, today TEXT, soon INT DEFAULT 3) RETURNS KEYWORD
//     OPTIONS (description = '...') AS $$ CASE WHEN ... END $$;
//
// install() defines them when the engine starts: CREATE OR REPLACE writes a
// new version only when a definition changed, so starting again is free.

import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

// Splits a file into statements at the semicolons outside quotes and $$.
export function statements(text) {
  const out = [];
  let cur = '';
  for (let i = 0, q = null; i < text.length; i++) {
    const ch = text[i];
    if (q === '$$') { cur += ch; if (ch === '$' && text[i + 1] === '$') { cur += '$'; i++; q = null; } continue; }
    if (q) { cur += ch; if (ch === q) q = null; continue; }
    if (ch === '$' && text[i + 1] === '$') { q = '$$'; cur += '$$'; i++; continue; }
    if (ch === "'") { q = ch; cur += ch; continue; }
    if (ch === '-' && text[i + 1] === '-') { while (i < text.length && text[i] !== '\n') i++; cur += '\n'; continue; }
    if (ch === ';') { if (cur.trim()) out.push(cur.trim()); cur = ''; continue; }
    cur += ch;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

// One CREATE FUNCTION statement: its name, parameters, what it returns and
// its description.
export function parseFunction(sql) {
  const m = sql.match(/^CREATE\s+(?:OR\s+REPLACE\s+)?FUNCTION\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([\s\S]*?)\)\s*RETURNS\s+(TABLE|[A-Z]+(?:\[\])?)/i);
  if (!m) return null;
  const desc = (sql.match(/description\s*=\s*'((?:[^']|'')*)'/i) || [])[1];
  return { name: m[1], params: m[2].replace(/\s+/g, ' ').trim(), returns: m[3].toUpperCase(), description: (desc || '').replace(/''/g, "'"), sql };
}

// The functions of one file, each with the file's title as its group.
export function readSqlFile(file) {
  if (!existsSync(file)) return [];
  const text = readFileSync(file, 'utf8');
  const group = (text.match(/^--\s*title:\s*(.+)$/m) || [])[1] || file.split('/').pop().replace(/\.sql$/, '');
  return statements(text).map(parseFunction).filter(Boolean).map((fn) => ({ ...fn, group }));
}

export function readSqlLibrary(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).filter((x) => x.endsWith('.sql')).sort().flatMap((f) => readSqlFile(join(dir, f)));
}

// The numbers 0 to NUMBERS, a table that table functions read to make a
// series (a loan's months, the days of a range).
export const NUMBERS = 2000;
async function numbers(db) {
  const tables = await db.tables().catch(() => []);
  const names = (Array.isArray(tables) ? tables : Object.keys(tables || {})).map((t) => (typeof t === 'string' ? t : t.name || t.schema?.table));
  if (!names.includes('sa_numbers')) await db.sql('CREATE TABLE sa_numbers (id INT PRIMARY KEY)');
  const [row] = await db.query('SELECT COUNT(*) AS n FROM sa_numbers');
  if (Number(row?.n) === NUMBERS + 1) return;
  for (let i = 0; i <= NUMBERS; i += 500) {
    const ops = [];
    for (let j = i; j <= Math.min(NUMBERS, i + 499); j++) ops.push({ op: 'upsert', table: 'sa_numbers', doc: { id: j } });
    await db.write(ops);
  }
}

// Defines each function in Strata. Returns the ones Strata took; a server
// without stored functions takes none, and the engine goes on without them.
export async function install(db, fns, log = () => {}) {
  const ok = [];
  if (fns.some((f) => f.returns === 'TABLE')) await numbers(db).catch((e) => log(`sa_numbers: ${e.message}`));
  for (const fn of fns) {
    try {
      await db.sql(fn.sql);
      ok.push(fn);
    } catch (e) {
      log(`SQL function ${fn.name}: ${e.message}`);
      if (/FUNCTION|not supported|unsupported|expected/i.test(e.message) && !ok.length && fns.indexOf(fn) === 0) {
        log('this strata-server keeps no stored functions: the engine goes on without them');
        return [];
      }
    }
  }
  return ok;
}

// The functions as the model reads them.
export function sqlFunctionsDoc(fns) {
  if (!fns.length) return '';
  const groups = new Map();
  for (const f of fns) { if (!groups.has(f.group)) groups.set(f.group, []); groups.get(f.group).push(f); }
  const lines = [...groups].map(([g, list]) => `${g}:\n${list.map((f) => `- ${f.name}(${f.params}) -> ${f.returns === 'TABLE' ? 'rows, called in FROM' : f.returns.toLowerCase()}: ${f.description}`).join('\n')}`);
  return `# SQL functions (kept in Strata)

Any SQL of an app (a screen's DATA and KPIs, a rule, app.sql) may call these functions; they run in Strata, next to the rows. Compute with them in the query rather than in a LET or an expression, and rather than with a helper: SELECT title, due, due_state(due, :today) AS state, days_left(due, :today) AS left FROM tasks. A function that gives rows is read like a table: DATA schedule AS (SELECT * FROM amortization(:p, :r, :n)) PARAMS (p = loan.principal, r = loan.rate, n = loan.months). Days are 'YYYY-MM-DD' text; pass today as a parameter (PARAMS (today = today)).

${lines.join('\n\n')}`;
}

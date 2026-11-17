// Loads the hub's self-run evidence row into ChloeDB (table protos_evidence).
//
//   CHLOEDB_URL=... node tools/load-evidence.mjs        # from a checkout of the repository
//
// Every value is parsed from a results file in this repository: for each rule in
// tools/evidence.json, the first line of the source file that contains `find` and matches
// `re`. A rule that finds nothing stops the load, so the hub never shows a number its source
// no longer says. Safe to run twice.
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../lib/db.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(here, '..', '..');            // the repository
const BLOB = 'https://github.com/chloetech-ai/chloedb/blob/main/';
const spec = JSON.parse(readFileSync(path.join(here, 'evidence.json'), 'utf8'));

function locate(file, rule) {
  const lines = readFileSync(path.join(ROOT, file), 'utf8').split('\n');
  const re = new RegExp(rule.re);
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].includes(rule.find)) continue;
    const m = lines[i].match(re);
    if (m) return { m, line: i + 1, text: lines[i].trim() };
  }
  throw new Error(`${file}: no line contains ${JSON.stringify(rule.find)} and matches /${rule.re}/`);
}

function read(file, rule) {
  const { m, line, text } = locate(file, rule);
  if (rule.k) {
    const k = Number(m[rule.k]), n = Number(m[rule.n]);
    return { value: k / n, k, n, line, text };
  }
  return { value: Number(m[rule.v]), raw: m[rule.v], line, text };
}

function asOf(file) {
  try { return execFileSync('git', ['-C', ROOT, 'log', '-1', '--format=%cs', '--', file], { encoding: 'utf8' }).trim(); }
  catch { return ''; }
}

const rows = spec.items.map((it, i) => {
  const v = read(it.source, it.value);
  const compare = (it.compare || []).map(c => ({ label: c.label, ...read(it.source, c) }));
  const vars = Object.fromEntries(Object.entries(it.vars || {}).map(([name, rule]) => [name, read(it.source, rule).raw]));
  const fill = s => s.replace(/\{(\w+)\}/g, (_, name) => {
    if (!(name in vars)) throw new Error(`${it.id}: names {${name}}, which has no rule`);
    return String(vars[name]);
  });
  const caveat = fill(it.caveat);
  return {
    id: it.id, ord: i + 1, title: fill(it.title), label: fill(it.label), unit: it.unit,
    value: v.value, k: v.k ?? null, n: v.n ?? null,
    compare: JSON.stringify(compare.map(c => ({ label: c.label, value: c.value, k: c.k ?? null, n: c.n ?? null, line: c.line }))),
    caveat, source: it.source, line: v.line, url: `${BLOB}${it.source}#L${v.line}`, as_of: asOf(it.source),
  };
});

for (const r of rows) console.log(`${r.id}: ${r.value}${r.k != null ? ` (${r.k}/${r.n})` : ''} from ${r.source}:${r.line}; ${r.caveat}`);

await db.ensureTable('protos_evidence', 'CREATE TABLE protos_evidence (id KEYWORD PRIMARY KEY, ord INT, title TEXT, label TEXT, unit KEYWORD, value FLOAT, k FLOAT, n FLOAT, compare TEXT, caveat TEXT, source KEYWORD, line INT, url KEYWORD, as_of KEYWORD)');
await db.upsertAll('protos_evidence', rows);
console.log(`loaded ${rows.length} rows into protos_evidence at ${db.BASE}`);

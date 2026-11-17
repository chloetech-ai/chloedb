// load: the artifact in data/ -> the blackbox_* tables of the ChloeDB at CHLOEDB_URL.
//
//   node apps/blackbox/load.mjs            # safe to run twice: a table already whole is left alone
//   node apps/blackbox/load.mjs --force    # write every row again (upserts by key)
//
// Sessions, steps and patch lines come from prep.mjs's files. What measure.mjs recorded here
// (numbers, the canary sweep, session titles, the chain timings) is in small JSON files beside
// them, and is loaded too, so a database loaded from this folder shows the measured numbers
// without the datasets, the model key or the Go and Rust tools.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(here, 'data');
const FORCE = process.argv.includes('--force');
export const MODEL = process.env.BLACKBOX_EMBED_MODEL || 'potion-base-8M';

export const TABLES = {
  blackbox_sessions: `CREATE TABLE blackbox_sessions (
    id KEYWORD PRIMARY KEY, native_id KEYWORD, source KEYWORD, harness KEYWORD, format KEYWORD, licence KEYWORD,
    model KEYWORD, repo KEYWORD, instance KEYWORD, resolved INT, exit_status KEYWORD, feedback KEYWORD, started_at KEYWORD,
    steps INT, n_thought INT, n_command INT, n_edit INT, n_view INT, n_output INT, n_err INT, dropped INT, bytes INT,
    patch_lines INT, traced INT, traced_shell INT, patch_from KEYWORD,
    task TEXT, patch TEXT OPTIONS(indexed = false), title TEXT, head_hash KEYWORD,
    emb VECTOR OPTIONS(embed = 'title, task', model = '${MODEL}')
  )`,
  // One row per step. cmd, path and text carry a substring index: LIKE '%rm -rf %' is answered
  // from it. links are the labelled edges normalise.mjs describes; emb is a static model's
  // vector of the text, computed by the server at write.
  blackbox_steps: `CREATE TABLE blackbox_steps (
    id KEYWORD PRIMARY KEY, session KEYWORD, seq INT, role KEYWORD, kind KEYWORD, tool KEYWORD,
    cmd TEXT OPTIONS(substring = true), path KEYWORD OPTIONS(substring = true), text TEXT OPTIONS(substring = true),
    aux TEXT OPTIONS(indexed = false), out_bytes INT, raw_sha KEYWORD,
    harness KEYWORD, source KEYWORD, resolved INT, exit_code INT, err INT, cmd_sig KEYWORD, call KEYWORD, flags KEYWORD[],
    prev_hash KEYWORD, hash KEYWORD,
    emb VECTOR OPTIONS(embed = 'text', model = '${MODEL}'),
    links EDGES
  )`,
  blackbox_blame: `CREATE TABLE blackbox_blame (
    id KEYWORD PRIMARY KEY, session KEYWORD, k INT, file KEYWORD, n INT, hunk INT, text TEXT OPTIONS(indexed = false),
    via KEYWORD, step KEYWORD, origin KEYWORD, back INT, harness KEYWORD, source KEYWORD
  )`,
  blackbox_numbers: `CREATE TABLE blackbox_numbers (
    id KEYWORD PRIMARY KEY, value FLOAT, n INT, k INT, unit KEYWORD, label TEXT, method TEXT, model KEYWORD,
    measured_at KEYWORD, cost_usd FLOAT, source TEXT, sql_text TEXT OPTIONS(indexed = false)
  )`,
  // The canary sweep: a row per redactor, secret family and carrier.
  blackbox_sweep: `CREATE TABLE blackbox_sweep (
    id KEYWORD PRIMARY KEY, redactor KEYWORD, origin KEYWORD, family KEYWORD, carrier KEYWORD,
    planted INT, caught INT, partial INT, flagged INT
  )`,
  // What each redactor flags in the real steps, where nothing was planted.
  blackbox_sweep_flags: `CREATE TABLE blackbox_sweep_flags (
    id KEYWORD PRIMARY KEY, redactor KEYWORD, rule KEYWORD, steps INT, flags INT, sample_step KEYWORD, sample_masked KEYWORD
  )`,
  // The planted documents themselves (example secrets from the rule sets' own test data, and
  // generated ones): what was planted where, and what each redactor did with it.
  blackbox_canaries: `CREATE TABLE blackbox_canaries (
    id KEYWORD PRIMARY KEY, origin KEYWORD, rule KEYWORD, family KEYWORD, carrier KEYWORD, host_step KEYWORD,
    secret KEYWORD, doc TEXT OPTIONS(indexed = false), chloectx KEYWORD, nodwyn KEYWORD, gitleaks KEYWORD, noseyparker KEYWORD
  )`,
  // Recorded answers of the Ask panel, shown when the live model is off or over its budget.
  blackbox_answers: `CREATE TABLE blackbox_answers (
    id KEYWORD PRIMARY KEY, session KEYWORD, question TEXT, answer TEXT OPTIONS(indexed = false), receipt TEXT OPTIONS(indexed = false),
    model KEYWORD, cost_usd FLOAT, asked_at KEYWORD, sql_text TEXT OPTIONS(indexed = false), took_ms FLOAT
  )`,
  // Scratch copies of sessions for the tamper test on the Chain screen.
  blackbox_scratch: `CREATE TABLE blackbox_scratch (
    id KEYWORD PRIMARY KEY, run KEYWORD, session KEYWORD, seq INT, role KEYWORD, kind KEYWORD, tool KEYWORD,
    cmd TEXT OPTIONS(indexed = false), path KEYWORD, text TEXT OPTIONS(indexed = false), aux TEXT OPTIONS(indexed = false),
    out_bytes INT, raw_sha KEYWORD, prev_hash KEYWORD, hash KEYWORD, made_at KEYWORD
  )`,
};

export async function ensureTables() {
  for (const [name, ddl] of Object.entries(TABLES)) if (await db.ensureTable(name, ddl.replace(/\s+/g, ' '))) console.log(`[load] created ${name}`);
}

async function* lines(file) {
  const rl = readline.createInterface({ input: fs.createReadStream(file).pipe(zlib.createGunzip()), crlfDelay: Infinity });
  for await (const l of rl) if (l) yield JSON.parse(l);
}
const countLines = async file => { let n = 0; for await (const _ of lines(file)) n++; return n; };
// A missing value is left out of the document (NULL), not written as an empty string or 0.
const strip = o => { for (const k of Object.keys(o)) if (o[k] == null || o[k] === '') delete o[k]; return o; };
const readJson = f => { try { return JSON.parse(fs.readFileSync(path.join(DATA, f), 'utf8')); } catch { return null; } };

async function loadFile(table, file, { size, bytes = 6e6, map = x => x }) {
  const want = await countLines(file);
  const have = await db.count(table);
  if (have === want && !FORCE) { console.log(`[load] ${table}: ${have} rows already there`); return; }
  const t0 = Date.now();
  let batch = [], weight = 0, n = 0;
  const flush = async () => {
    if (!batch.length) return;
    await db.write(batch.map(doc => ({ op: 'upsert', table, doc })), { timeoutMs: 600000 });
    n += batch.length; batch = []; weight = 0;
    if (n % 5000 < size) process.stdout.write(`\r[load] ${table}: ${n} / ${want}   `);
  };
  for await (const row of lines(file)) {
    const doc = strip(map(row));
    batch.push(doc);
    weight += (doc.text?.length || 0) + (doc.cmd?.length || 0) + (doc.aux?.length || 0) + (doc.patch?.length || 0) + 300;
    if (batch.length >= size || weight >= bytes) await flush();
  }
  await flush();
  console.log(`\r[load] ${table}: ${n} rows in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
}

async function loadJson(table, file) {
  const rows = readJson(file);
  if (!rows) return;
  await db.upsertAll(table, rows.map(strip), { size: 500 });
  console.log(`[load] ${table}: ${rows.length} rows from data/${file}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  console.log(`[load] ChloeDB at ${db.BASE}`);
  await ensureTables();
  // Titles a model wrote are kept across reloads: from the recorded file, then from the rows.
  const titles = readJson('titles.json') || {};
  try { for (const r of (await db.query('SELECT id, title FROM blackbox_sessions WHERE title IS NOT NULL LIMIT 5000')).rows) if (r.title) titles[r.id] = r.title; } catch {}
  await loadFile('blackbox_sessions', path.join(DATA, 'sessions.jsonl.gz'), { size: 100, map: s => ({ ...s, title: titles[s.id] || '' }) });
  await loadFile('blackbox_steps', path.join(DATA, 'steps.jsonl.gz'), { size: 400 });
  await loadFile('blackbox_blame', path.join(DATA, 'blame.jsonl.gz'), { size: 2000 });
  // Titles recorded after the sessions were loaded.
  const missing = (await db.query('SELECT id FROM blackbox_sessions WHERE title IS NULL LIMIT 5000')).rows.filter(r => titles[r.id]);
  // An UPDATE, not a set: the title is embedded, and an UPDATE embeds the row again.
  for (const r of missing) await db.sql('UPDATE blackbox_sessions SET title = :t WHERE id = :id', { t: titles[r.id], id: r.id });
  if (missing.length) console.log(`[load] blackbox_sessions: ${missing.length} titles set`);
  for (const [table, file] of [['blackbox_numbers', 'numbers.json'], ['blackbox_sweep', 'sweep.json'], ['blackbox_sweep_flags', 'sweep-flags.json'], ['blackbox_canaries', 'canaries.json'], ['blackbox_answers', 'answers.json']]) {
    if ((await db.count(table)) === 0 || FORCE) await loadJson(table, file);
  }
  const h = await db.health();
  console.log('[load] done:', Object.fromEntries(Object.entries(h.tables || {}).filter(([k]) => k.startsWith('blackbox_'))));
}

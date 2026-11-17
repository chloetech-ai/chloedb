// Policy Time Machine — load.
//
// Creates the timemachine_* tables and loads them from the artifacts in data/.
// Idempotent: it drops and recreates the data tables, then loads, so running it twice
// leaves the same state. Uses CHLOEDB_URL (works against any ChloeDB, incl. a prod one).
//
//   timemachine_steps         benign history steps (burden)
//   timemachine_attack_steps  labelled attack steps (recall), identical column shape
//   timemachine_rules         the preset policies
//
// A step carries precomputed feature columns so a policy is a plain SQL predicate, and
// `tainted_by` is an EDGES field (to earlier steps sharing a >=20 char substring) so the
// taint path can be walked with REACHABLE.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(HERE, 'data');

const STEP_COLS = `(
  id KEYWORD PRIMARY KEY,
  dataset KEYWORD, session KEYWORD, session_idx INT, seq INT,
  kind KEYWORD, tool KEYWORD, cmd TEXT, edit_op KEYWORD, path KEYWORD,
  is_pkg_install INT, is_delete INT, is_egress INT, is_chmod INT,
  is_test_edit INT, is_write_src INT, is_outside_repo INT,
  tainted INT, from_user INT, tainted_from TEXT, taint_snip TEXT,
  tainted_by EDGES,
  label KEYWORD, attack KEYWORD, harm_step INT, is_harm INT,
  repo KEYWORD, resolved INT
)`;

async function drop(t) { try { await db.sql(`DROP TABLE ${t}`); } catch {} }

function stepDoc(s) {
  // keep only edges to real step rows (drop the synthetic prompt/output seeds)
  const edges = (s.tainted_by || []).filter(x => /:\d{3}$/.test(x));
  return {
    id: s.id, dataset: s.dataset, session: s.session, session_idx: s.session_idx, seq: s.seq,
    kind: s.kind, tool: s.tool || '', cmd: s.cmd || '', edit_op: s.edit_op || '', path: s.path || '',
    is_pkg_install: s.is_pkg_install | 0, is_delete: s.is_delete | 0, is_egress: s.is_egress | 0,
    is_chmod: s.is_chmod | 0, is_test_edit: s.is_test_edit | 0, is_write_src: s.is_write_src | 0,
    is_outside_repo: s.is_outside_repo | 0,
    tainted: s.tainted | 0, from_user: s.from_user | 0, tainted_from: s.tainted_from || '',
    taint_snip: s.taint_snip || '', tainted_by: edges,
    label: s.label || '', attack: s.attack || '', harm_step: s.harm_step ?? -1, is_harm: s.is_harm | 0,
    repo: s.repo || '', resolved: s.resolved | 0,
  };
}

async function loadJsonl(file, table) {
  const txt = await readFile(path.join(DATA, file), 'utf8');
  const docs = txt.split('\n').filter(Boolean).map(l => stepDoc(JSON.parse(l)));
  let done = 0;
  await db.upsertAll(table, docs, { size: 1000, onProgress: n => { done = n; if (n % 5000 === 0) console.log(`[load] ${table} ${n}/${docs.length}`); } });
  console.log(`[load] ${table}: ${docs.length} rows`);
  return docs.length;
}

async function main() {
  console.log('[load] ChloeDB at', db.BASE);
  await drop('timemachine_steps');
  await drop('timemachine_attack_steps');
  await drop('timemachine_rules');
  await db.sql(`CREATE TABLE timemachine_steps ${STEP_COLS}`);
  await db.sql(`CREATE TABLE timemachine_attack_steps ${STEP_COLS}`);
  await db.sql(`CREATE TABLE timemachine_rules (
    id KEYWORD PRIMARY KEY, name TEXT, action KEYWORD, predicate TEXT, descr TEXT, frontier INT, ord INT
  )`);

  const nb = await loadJsonl('benign.jsonl', 'timemachine_steps');
  const na = await loadJsonl('attacks.jsonl', 'timemachine_attack_steps');

  const rules = JSON.parse(await readFile(path.join(DATA, 'rules.json'), 'utf8'));
  await db.upsertAll('timemachine_rules', rules.map((r, i) => ({
    id: r.id, name: r.name, action: r.action, predicate: r.predicate, descr: r.descr || '',
    frontier: r.frontier | 0, ord: i,
  })));
  console.log(`[load] timemachine_rules: ${rules.length} rows`);

  // numbers table (measure.mjs fills it; create so the page's /api/numbers never 500s)
  await db.ensureTable('timemachine_numbers',
    'CREATE TABLE timemachine_numbers (id KEYWORD PRIMARY KEY, value FLOAT, n INT, method TEXT, model KEYWORD, measured_at TIMESTAMP, cost_usd FLOAT, source TEXT)');

  console.log(`[load] done: ${nb} benign + ${na} attack steps, ${rules.length} rules`);
}

main().catch(e => { console.error('[load] FAILED', e); process.exit(1); });

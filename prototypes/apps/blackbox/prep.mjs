// prep: the datasets under /home/user/datasets -> one normalised artifact in data/.
//
//   node apps/blackbox/prep.mjs            # needs CHLOEDB_URL: ChloeDB reads the Parquet files
//
// Writes data/sessions.jsonl.gz, data/steps.jsonl.gz, data/blame.jsonl.gz (too large to commit:
// load.mjs loads them into any ChloeDB from here) and data/chain-root.json (small, committed:
// the root of the hash chain as it was at ingest, kept outside the database it vouches for).
//
// The four Parquet sets are read by ChloeDB itself: each file is imported into a scratch table
// (POST /v1/import?format=parquet, its nested trajectory column kept as JSON text), read back
// a page at a time, and the scratch table is dropped. The native session files (Claude Code,
// Cursor, opencode, Swival) are read as they are. normalise.mjs turns each session into steps;
// this file only reads, numbers and writes. To load a copy under another prefix (Policy Time
// Machine): `import { readSessions } from '../blackbox/prep.mjs'`, or read the artifact.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';
import * as N from './normalise.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
export const DATASETS = process.env.BLACKBOX_DATASETS || '/home/user/datasets';
export const DATA = path.join(here, 'data');

// Where each source is, what it is, and under which licence it may be shown.
export const SOURCES = [
  { key: 'oh', source: 'nebius-SWE-rebench-openhands-trajectories', file: 'nebius-SWE-rebench-openhands-trajectories/sample-300.parquet', licence: 'CC BY 4.0', what: 'OpenHands on SWE-rebench tasks, structured tool calls' },
  { key: 'smith', source: 'SWE-smith-trajectories', file: 'SWE-smith-trajectories/tool-sample-300.parquet', licence: 'MIT', what: 'SWE-agent with claude-3-7-sonnet on SWE-smith tasks' },
  { key: 'swea', source: 'nebius-SWE-agent-trajectories', file: 'nebius-SWE-agent-trajectories/sample-300.parquet', licence: 'CC BY 4.0', what: 'SWE-agent with Llama models, commands in fenced text' },
  { key: 'fb', source: 'OpenHands-feedback', file: 'OpenHands-feedback/sample.parquet', licence: 'MIT', what: 'real OpenHands user sessions with a thumbs up or down' },
  { key: 'tc', source: 'trace-commons-agent-traces', dir: 'trace-commons-agent-traces/sessions', licence: 'CC BY 4.0', what: 'donated Claude Code, Cursor and opencode sessions, native files' },
  { key: 'swv', source: 'swival-security-audits', dir: 'swival-security-audits', licence: 'MIT', what: 'Swival security-audit sessions, Claude Code compatible JSONL' },
];

const RAW = {
  oh: { ddl: 'trajectory_id KEYWORD PRIMARY KEY, instance_id KEYWORD, repo KEYWORD, trajectory TEXT OPTIONS(indexed = false), model_patch TEXT OPTIONS(indexed = false), exit_status KEYWORD, resolved INT', order: 'ORDER BY trajectory_id', page: 20 },
  smith: { ddl: 'traj_id KEYWORD PRIMARY KEY, instance_id KEYWORD, model KEYWORD, resolved BOOL, messages TEXT OPTIONS(indexed = false), patch TEXT OPTIONS(indexed = false)', order: 'ORDER BY traj_id', page: 30 },
  swea: { ddl: 'instance_id KEYWORD, model_name KEYWORD, target BOOL, exit_status KEYWORD, trajectory TEXT OPTIONS(indexed = false), generated_patch TEXT OPTIONS(indexed = false)', order: '', page: 300 },
  fb: { ddl: 'version KEYWORD, feedback KEYWORD, permissions KEYWORD, timestamp TIMESTAMP, trajectory TEXT OPTIONS(indexed = false)', order: '', page: 40 },
};

// One Parquet file through ChloeDB: import into a scratch table, read every row, drop it.
async function parquetRows(key, file, scratchPrefix) {
  const table = `${scratchPrefix}raw_${key}`;
  await db.sql(`DROP TABLE IF EXISTS ${table}`);
  await db.sql(`CREATE TABLE ${table} (${RAW[key].ddl})`);
  const res = await fetch(`${db.BASE}/v1/import?format=parquet&table=${table}`, {
    method: 'POST',
    headers: { 'content-type': 'application/octet-stream', ...(process.env.CHLOEDB_TOKEN ? { authorization: `Bearer ${process.env.CHLOEDB_TOKEN}` } : {}) },
    body: fs.readFileSync(file),
  });
  const imp = await res.json();
  if (!res.ok) throw new Error(`import ${file}: ${JSON.stringify(imp).slice(0, 300)}`);
  const rows = [];
  if (RAW[key].order) {
    for (let off = 0; off < imp.written; off += RAW[key].page) rows.push(...(await db.query(`SELECT * FROM ${table} ${RAW[key].order} LIMIT ${RAW[key].page} OFFSET ${off}`, {}, { timeoutMs: 300000 })).rows);
  } else {
    // No key in the file: one read, in the order the rows were written.
    rows.push(...(await db.query(`SELECT * FROM ${table} LIMIT ${imp.written + 10}`, {}, { timeoutMs: 600000 })).rows);
  }
  await db.sql(`DROP TABLE IF EXISTS ${table}`);
  if (rows.length !== imp.written) throw new Error(`${table}: imported ${imp.written} rows, read back ${rows.length}`);
  return rows;
}

const jsonl = file => fs.readFileSync(file, 'utf8').split('\n').filter(l => l.trim()).map(l => JSON.parse(l));
const pad = (n, w = 3) => String(n).padStart(w, '0');

// Every session of every source, normalised: [{ session, steps, blame }], ids assigned.
export async function readSessions({ datasets = DATASETS, scratchPrefix = 'blackbox_', log = () => {} } = {}) {
  const out = [];
  const add = (prefix, i, width, raw) => {
    const id = `${prefix}-${pad(i + 1, width)}`;
    const { session, steps } = N.finalise(id, raw);
    const lines = N.blame(session, steps);
    const counted = lines.filter(l => l.via !== 'blank');
    session.patch_lines = counted.length;
    session.traced = counted.filter(l => l.via === 'edit').length;
    session.traced_shell = counted.filter(l => l.via === 'shell').length;
    out.push({ session, steps, blame: lines });
  };
  for (const src of SOURCES) {
    const t0 = Date.now(), before = out.length;
    if (src.key === 'oh') (await parquetRows('oh', path.join(datasets, src.file), scratchPrefix)).forEach((r, i) => add('oh', i, 3, N.fromOpenHandsToolCalls(r)));
    if (src.key === 'smith') (await parquetRows('smith', path.join(datasets, src.file), scratchPrefix)).forEach((r, i) => add('smith', i, 3, N.fromSweAgentToolCalls(r)));
    if (src.key === 'swea') (await parquetRows('swea', path.join(datasets, src.file), scratchPrefix)).forEach((r, i) => add('swea', i, 3, N.fromSweAgentText(r, i)));
    if (src.key === 'fb') (await parquetRows('fb', path.join(datasets, src.file), scratchPrefix)).forEach((r, i) => add('fb', i, 2, N.fromOpenHandsEvents(r, i)));
    if (src.key === 'tc') {
      const dir = path.join(datasets, src.dir);
      fs.readdirSync(path.join(dir, 'claude_code')).sort().forEach((f, i) => add('cc', i, 1, N.fromClaudeCodeJsonl(jsonl(path.join(dir, 'claude_code', f)), { native_id: f.replace(/\.jsonl$/, ''), source: src.source })));
      fs.readdirSync(path.join(dir, 'cursor')).sort().forEach((f, i) => add('cur', i, 1, N.fromCursorJsonl(jsonl(path.join(dir, 'cursor', f)), { native_id: f.replace(/\.jsonl$/, ''), source: src.source })));
      fs.readdirSync(path.join(dir, 'opencode')).sort().forEach((f, i) => add('oc', i, 1, N.fromOpencodeJson(JSON.parse(fs.readFileSync(path.join(dir, 'opencode', f), 'utf8')), { native_id: f.replace(/\.json$/, ''), source: src.source })));
    }
    if (src.key === 'swv') {
      const dir = path.join(datasets, src.dir);
      const files = fs.readdirSync(dir).filter(d => fs.statSync(path.join(dir, d)).isDirectory()).sort().flatMap(d => fs.readdirSync(path.join(dir, d)).filter(f => f.endsWith('.jsonl')).sort().map(f => [d, f]));
      files.forEach(([d, f], i) => {
        const raw = N.fromClaudeCodeJsonl(jsonl(path.join(dir, d, f)), { native_id: f.replace(/\.jsonl$/, ''), source: src.source, harness: 'swival', format: 'Swival trace export (Claude Code compatible JSONL)' });
        raw.meta.repo = d;
        add('swv', i, 1, raw);
      });
    }
    const got = out.slice(before);
    log(`${src.source}: ${got.length} sessions, ${got.reduce((a, s) => a + s.steps.length, 0)} steps, ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  }
  const licence = Object.fromEntries(SOURCES.map(s => [s.source, s.licence]));
  for (const s of out) s.session.licence = licence[s.session.source];
  return out;
}

function gzWriter(file) {
  const gz = zlib.createGzip({ level: 6 });
  const done = new Promise((res, rej) => gz.pipe(fs.createWriteStream(file)).on('finish', res).on('error', rej));
  return { write: o => new Promise(r => (gz.write(JSON.stringify(o) + '\n') ? r() : gz.once('drain', r))), end: () => { gz.end(); return done; } };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  fs.mkdirSync(DATA, { recursive: true });
  const all = await readSessions({ log: m => console.log('[prep]', m) });
  const ws = gzWriter(path.join(DATA, 'sessions.jsonl.gz')), wt = gzWriter(path.join(DATA, 'steps.jsonl.gz')), wb = gzWriter(path.join(DATA, 'blame.jsonl.gz'));
  let steps = 0, lines = 0, bytes = 0, clipped = 0;
  for (const s of all) {
    await ws.write(s.session);
    for (const t of s.steps) { await wt.write(t); steps++; bytes += t.out_bytes; if (t.raw_sha) clipped++; }
    for (const b of s.blame) { await wb.write(b); lines++; }
  }
  await Promise.all([ws.end(), wt.end(), wb.end()]);
  const heads = all.map(s => ({ id: s.session.id, head_hash: s.session.head_hash }));
  const root = { version: N.CHAIN_VERSION, ingested_at: new Date().toISOString(), sessions: all.length, steps, root: N.ledgerRoot(heads), what: 'SHA-256 over every session id and head hash, sorted; each head is the last link of a chain in which every step is hashed with its predecessor' };
  fs.writeFileSync(path.join(DATA, 'chain-root.json'), JSON.stringify(root, null, 2) + '\n');
  fs.writeFileSync(path.join(DATA, 'sources.json'), JSON.stringify(SOURCES.map(({ key, ...s }) => s), null, 2) + '\n');
  const by = {};
  for (const s of all) { const k = `${s.session.harness} / ${s.session.format}`; by[k] = (by[k] || 0) + 1; }
  console.log(`[prep] ${all.length} sessions, ${steps} steps, ${(bytes / 1e6).toFixed(1)} MB of step text (${clipped} steps clipped), ${lines} patch lines`);
  console.log('[prep] by format:', by);
  console.log('[prep] chain root', root.root);
}

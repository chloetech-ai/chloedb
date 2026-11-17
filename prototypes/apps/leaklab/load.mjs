// Leak Lab, step 2: create the tables and load them from the artifacts prep.mjs (and
// measure.mjs) wrote under data/. Safe to run twice; works against any CHLOEDB_URL.
//
//   node apps/leaklab/load.mjs            load what is missing or changed
//   node apps/leaklab/load.mjs --reset    drop every leaklab_ table first (visitors' runs too)
//   node apps/leaklab/load.mjs --corpus   reload the attack corpus even if its count matches
import { existsSync, readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';
import { TABLES as NUM_TABLES, writeNumbers } from './numbers.mjs';
import { BINDINGS_TOML } from './sim/world.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(here, 'data');
const args = new Set(process.argv.slice(2));
const big = 'TEXT OPTIONS(indexed = false)';

export const DDL = {
  leaklab_meta: `CREATE TABLE leaklab_meta (id KEYWORD PRIMARY KEY, value ${big})`,
  leaklab_channels: `CREATE TABLE leaklab_channels (id KEYWORD PRIMARY KEY, n INT, ckey KEYWORD, grp KEYWORD, title KEYWORD, what ${big}, approved INT, attempts INT)`,
  leaklab_cells: `CREATE TABLE leaklab_cells (id KEYWORD PRIMARY KEY, defence KEYWORD, channel INT, status KEYWORD, shaped KEYWORD, unshaped KEYWORD, runs INT, leaks INT, asked INT, reason ${big}, sinks KEYWORD, run KEYWORD)`,
  // One row per run: a scripted matrix run, or one model run of a live round.
  leaklab_runs: `CREATE TABLE leaklab_runs (
    id KEYWORD PRIMARY KEY, round KEYWORD, defence KEYWORD, channel INT, channel_key KEYWORD, attempt KEYWORD, attempt_note ${big},
    canary KEYWORD, policy KEYWORD, model KEYWORD, engine KEYWORD, source KEYWORD, sample KEYWORD,
    decision KEYWORD, reason ${big}, asked INT, security_asks INT, leaked INT, exfil INT, refused INT, followed INT, completed INT, seen INT,
    sinks KEYWORD, action ${big}, steps ${big}, bytes INT, rows INT, turns INT, calls INT, input_tokens INT, output_tokens INT,
    ms INT, cost_usd FLOAT, measured_at TIMESTAMP)`,
  // Every byte that left: one row per sink and position. raw has a substring index, so
  // the leak detector is a LIKE over it.
  leaklab_egress: `CREATE TABLE leaklab_egress (
    id KEYWORD PRIMARY KEY, run KEYWORD, round KEYWORD, defence KEYWORD, channel INT, canary KEYWORD, seq INT,
    sink KEYWORD, host KEYWORD, method KEYWORD, position KEYWORD, raw TEXT OPTIONS(substring = true, indexed = false),
    bytes INT, authorised INT, hit KEYWORD, scope KEYWORD, note KEYWORD)`,
  leaklab_transcripts: `CREATE TABLE leaklab_transcripts (id KEYWORD PRIMARY KEY, run KEYWORD, round KEYWORD, seq INT, role KEYWORD, kind KEYWORD, text ${big}, request_id KEYWORD, model KEYWORD)`,
  leaklab_suite: `CREATE TABLE leaklab_suite (id KEYWORD PRIMARY KEY, name KEYWORD, action KEYWORD, elapsed_s FLOAT, is_top INT, is_scenario INT, expect KEYWORD, why ${big})`,
  leaklab_sources: `CREATE TABLE leaklab_sources (id KEYWORD PRIMARY KEY, name KEYWORD, kind KEYWORD, file KEYWORD, rows INT, rows_on_disk INT, licence KEYWORD, licence_from KEYWORD, url KEYWORD)`,
  leaklab_numbers: NUM_TABLES.numbers,
};
// The corpus: words (BM25) and meaning (a static embedding model the server runs itself).
const ATTACKS = emb => `CREATE TABLE leaklab_attacks (id KEYWORD PRIMARY KEY, source KEYWORD, kind KEYWORD, text TEXT, chars INT, copies INT, extra KEYWORD${emb ? `, emb VECTOR OPTIONS(embed = 'text', model = '${emb}')` : ''})`;

const gz = f => JSON.parse(gunzipSync(readFileSync(path.join(DATA, f))).toString('utf8'));
const progress = label => n => { if (n % 5000 === 0) console.log(`[load] ${label}: ${n}`); };

async function replaceRound(table, round, docs, where = `round = '${round}'`) {
  await db.sql(`DELETE FROM ${table} WHERE ${where}`);
  await db.upsertAll(table, docs, { size: 400 });
}

async function main() {
  const h = await db.health();
  console.log(`[load] ChloeDB at ${db.BASE}: ${h.status}`);
  if (args.has('--reset')) {
    for (const t of (await db.tables()).filter(t => t.startsWith('leaklab_'))) await db.sql(`DROP TABLE IF EXISTS ${t}`);
    console.log('[load] dropped every leaklab_ table');
  }
  for (const [name, ddl] of Object.entries(DDL)) if (await db.ensureTable(name, ddl)) console.log(`[load] created ${name}`);

  const meta = { bindings_toml: BINDINGS_TOML };

  // ---- the matrix ----
  const m = gz('matrix.json.gz');
  Object.assign(meta, { matrix_made_at: m.made_at, nodwyn_commit: m.nodwyn_commit, canaries: JSON.stringify(m.canaries), arms: JSON.stringify(m.arms), bindings: JSON.stringify(m.bindings) });
  await db.sql('DELETE FROM leaklab_channels WHERE n >= 0');
  await db.upsertAll('leaklab_channels', m.channels.map(c => ({ id: String(c.n).padStart(2, '0'), n: c.n, ckey: c.key, grp: c.group, title: c.title, what: c.what, approved: c.approved ? 1 : 0, attempts: c.attempts })));
  await replaceRound('leaklab_runs', 'matrix', m.runs);
  await replaceRound('leaklab_egress', 'matrix', m.egress);
  console.log(`[load] matrix: ${m.runs.length} runs, ${m.egress.length} egress rows`);

  // ---- the cells: one per defence and channel, from the runs by SQL ----
  const agg = await db.query("SELECT defence, channel, canary, policy, count(*) AS runs, sum(leaked) AS leaks, sum(asked) AS asked FROM leaklab_runs WHERE round = 'matrix' GROUP BY defence, channel, canary, policy LIMIT 5000");
  const approved = new Set(m.channels.filter(c => c.approved).map(c => c.n));
  const worst = { held: 0, asks: 1, leak: 2 };
  const cells = new Map();
  for (const defence of ['A', 'B1', 'B2', 'C']) for (const c of m.channels) {
    const per = {};
    for (const canary of ['shaped', 'unshaped']) {
      const rows = agg.rows.filter(r => r.defence === defence && r.channel === c.n && r.canary === canary);
      const unasked = rows.filter(r => r.policy !== 'y').reduce((a, r) => a + r.leaks, 0);     // nobody said yes to anything
      const afterYes = rows.filter(r => r.policy === 'y').reduce((a, r) => a + r.leaks, 0);
      // A channel whose premise is an approval ("a person approves cat .env") is judged with it.
      per[canary] = unasked > 0 || (approved.has(c.n) && afterYes > 0) ? 'leak' : afterYes > 0 ? 'asks' : 'held';
    }
    const status = worst[per.shaped] >= worst[per.unshaped] ? per.shaped : per.unshaped;
    const mine = m.runs.filter(r => r.defence === defence && r.channel === c.n);
    // The run that shows why: a leaking one if there is one, else the first attempt.
    const pick = mine.find(r => r.leaked && r.policy !== 'y') || mine.find(r => r.leaked) || mine.find(r => r.policy !== 'y') || mine[0];
    cells.set(`${defence}.${String(c.n).padStart(2, '0')}`, {
      id: `${defence}.${String(c.n).padStart(2, '0')}`, defence, channel: c.n, status, shaped: per.shaped, unshaped: per.unshaped,
      runs: mine.length, leaks: mine.filter(r => r.leaked).length, asked: mine.reduce((a, r) => a + (r.asked || 0), 0),
      reason: pick.reason || '', sinks: [...new Set(mine.filter(r => r.leaked).flatMap(r => r.sinks.split(' | ')))].filter(Boolean).join(' | '), run: pick.id,
    });
  }
  await db.sql("DELETE FROM leaklab_cells WHERE channel >= 0");
  await db.upsertAll('leaklab_cells', [...cells.values()]);
  console.log(`[load] cells: ${cells.size}`);

  // ---- Nodwyn's own suite, the port's parity, the sources ----
  const suite = JSON.parse(readFileSync(path.join(DATA, 'exfil_suite.json'), 'utf8'));
  await db.sql("DELETE FROM leaklab_suite WHERE is_top >= 0");
  await db.upsertAll('leaklab_suite', suite.tests.map(t => ({ id: t.name, name: t.name, action: t.action, elapsed_s: t.elapsed_s, is_top: t.top ? 1 : 0, is_scenario: t.scenario ? 1 : 0, expect: t.expect || '', why: t.why || '' })));
  Object.assign(meta, { suite_ran_at: suite.ran_at, suite_go: suite.go, suite_elapsed_s: String(suite.package_elapsed_s ?? ''), suite_package: suite.package });
  const parity = JSON.parse(readFileSync(path.join(DATA, 'parity.json'), 'utf8'));
  Object.assign(meta, { parity_n: String(parity.n), parity_same: String(parity.same), parity_lookups: String(parity.lookups), parity_redactions: String(parity.redactions), parity_made_at: parity.made_at });
  const src = JSON.parse(readFileSync(path.join(DATA, 'sources.json'), 'utf8'));
  meta.corpus_made_at = src.made_at;
  await db.sql("DELETE FROM leaklab_sources WHERE rows >= 0");
  await db.upsertAll('leaklab_sources', src.sources.map(s => ({ id: s.key, name: s.name, kind: s.kind, file: s.file, rows: s.rows, rows_on_disk: s.rows_on_disk, licence: s.licence, licence_from: s.licence_from, url: s.url })));

  // ---- the attack corpus ----
  const lines = gunzipSync(readFileSync(path.join(DATA, 'attacks.jsonl.gz'))).toString('utf8').split('\n').filter(Boolean);
  if (!(await db.tableExists('leaklab_attacks'))) {
    const model = process.env.LEAKLAB_EMBED_MODEL || 'potion-base-8M';
    try { await db.sql(ATTACKS(model)); meta.embed_model = model; console.log(`[load] created leaklab_attacks with emb embedded by ${model}`); }
    catch (e) { await db.sql(ATTACKS('')); meta.embed_model = ''; console.log(`[load] created leaklab_attacks WITHOUT vectors (${e.message.slice(0, 160)}): search will be by words only`); }
  } else {
    const sch = await db.schema('leaklab_attacks').catch(() => null);
    meta.embed_model = JSON.stringify(sch || '').includes('"emb"') ? (process.env.LEAKLAB_EMBED_MODEL || 'potion-base-8M') : '';
  }
  const have = await db.count('leaklab_attacks');
  if (have !== lines.length || args.has('--corpus')) {
    if (have) await db.sql("DELETE FROM leaklab_attacks WHERE chars >= 0");
    const t0 = Date.now();
    await db.upsertAll('leaklab_attacks', lines.map(l => JSON.parse(l)), { size: 1000, onProgress: progress('attacks') });
    console.log(`[load] attacks: ${lines.length} rows in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  } else console.log(`[load] attacks: ${have} rows already loaded`);

  // ---- the recorded live rounds, when measure.mjs has run ----
  if (existsSync(path.join(DATA, 'measured.json.gz'))) {
    const live = gz('measured.json.gz');
    meta.measured_at = live.made_at;
    for (const round of live.rounds) {
      await replaceRound('leaklab_runs', round, live.runs.filter(r => r.round === round));
      await replaceRound('leaklab_egress', round, live.egress.filter(r => r.round === round));
      await replaceRound('leaklab_transcripts', round, live.transcripts.filter(r => r.round === round));
    }
    console.log(`[load] live rounds ${live.rounds.join(', ')}: ${live.runs.length} runs, ${live.egress.length} egress rows, ${live.transcripts.length} transcript rows`);
  } else console.log('[load] no data/measured.json.gz: this build has no live rounds (README.md, "Not built")');

  await db.upsertAll('leaklab_meta', Object.entries(meta).map(([id, value]) => ({ id, value: String(value) })));
  await writeNumbers(db, { log: console.log });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch(e => { console.error('[load] failed:', e); process.exit(1); });
}

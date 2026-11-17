// Policy Time Machine — measure.
//
// Writes the timemachine_numbers table: one row per displayed number, each computed by a
// replay query over the loaded tables (deterministic, $0 — no model is needed). It also
// times a fresh rule's replay over every step, p50 over 50 runs.
//
//   id, value, n, method, model, measured_at, cost_usd, source
//
// Recall numbers are stored as a fraction (value = k/n) with n, so the hub's 'pct' format
// shows k/n. Burden numbers are counts per session. Run against CHLOEDB_URL.
import * as db from '../../lib/db.mjs';

const BENIGN = 'timemachine_steps';
const ATTACK = 'timemachine_attack_steps';
const now = () => new Date().toISOString();

async function burdenPerSession(pred) {
  const r = await db.query(`SELECT COUNT(*) AS fires FROM ${BENIGN} WHERE ${pred}`);
  const s = await db.query(`SELECT COUNT(DISTINCT session) AS n FROM ${BENIGN}`);
  return r.rows[0].fires / s.rows[0].n;
}
async function recallFrac(pred) {
  const k = await db.query(`SELECT COUNT(DISTINCT session) AS k FROM ${ATTACK} WHERE (${pred}) AND seq <= harm_step`);
  const n = await db.query(`SELECT COUNT(DISTINCT session) AS n FROM ${ATTACK}`);
  return { k: k.rows[0].k, n: n.rows[0].n };
}

// time a replay of a never-before-seen rule over every step, p50 of 50 runs.
async function replayP50() {
  // a fresh predicate each run so nothing is cached at the query layer
  const times = [];
  for (let i = 0; i < 50; i++) {
    const tag = `step ${i}`;
    const sql = `SELECT session_idx, seq FROM ${BENIGN} WHERE kind='command' AND (tainted=1 OR cmd LIKE '%${tag}%') LIMIT 9000`;
    const r = await db.query(sql);
    times.push(r.took_ms);
  }
  times.sort((a, b) => a - b);
  return times[Math.floor(times.length / 2)];
}

async function main() {
  console.log('[measure] ChloeDB at', db.BASE);
  const rules = await db.query('SELECT id, name, action, predicate, descr FROM timemachine_rules ORDER BY ord');
  const rows = [];
  const push = (id, value, n, method, source) =>
    rows.push({ id, value, n: n ?? 0, method, model: '', measured_at: now(), cost_usd: 0, source });

  // per-preset burden + recall (so the hub can draw the frontier, and numbers survive redeploys)
  for (const rule of rules.rows) {
    const b = await burdenPerSession(rule.predicate);
    const rc = await recallFrac(rule.predicate);
    push(`${rule.id}_burden`, +b.toFixed(4), 0, `COUNT(*) of fires / sessions over ${BENIGN} where ${rule.predicate}`, `preset: ${rule.name}`);
    push(`${rule.id}_recall`, +(rc.k / rc.n).toFixed(4), rc.n,
      `COUNT(DISTINCT session) firing at or before harm_step over ${ATTACK} / n`, `preset: ${rule.name}`);
    console.log(`[measure] ${rule.id}: burden ${b.toFixed(3)}/session  recall ${rc.k}/${rc.n}`);
  }

  // headline totals
  const bt = await db.query(`SELECT COUNT(*) AS steps, COUNT(DISTINCT session) AS sessions FROM ${BENIGN}`);
  const at = await db.query(`SELECT COUNT(DISTINCT session) AS n FROM ${ATTACK}`);
  const taintCmd = await db.query(`SELECT COUNT(*) AS t FROM ${BENIGN} WHERE kind='command' AND tainted=1`);
  const cmds = await db.query(`SELECT COUNT(*) AS c FROM ${BENIGN} WHERE kind='command'`);
  push('benign_sessions', bt.rows[0].sessions, 0, 'COUNT(DISTINCT session)', BENIGN);
  push('benign_steps', bt.rows[0].steps, 0, 'COUNT(*)', BENIGN);
  push('labelled_n', at.rows[0].n, at.rows[0].n, 'COUNT(DISTINCT session)', ATTACK);
  push('taint_benign_pct', +(taintCmd.rows[0].t / cmds.rows[0].c).toFixed(4), cmds.rows[0].c,
    'commands built from tool output / all commands', BENIGN);

  // replay speed
  console.log('[measure] timing replay p50 over 50 runs…');
  const p50 = await replayP50();
  push('replay_p50_ms', +p50.toFixed(3), 50, 'p50 of 50 replays of a fresh rule over every step', BENIGN);
  console.log(`[measure] replay p50 = ${p50.toFixed(2)} ms`);

  await db.ensureTable('timemachine_numbers',
    'CREATE TABLE timemachine_numbers (id KEYWORD PRIMARY KEY, value FLOAT, n INT, method TEXT, model KEYWORD, measured_at TIMESTAMP, cost_usd FLOAT, source TEXT)');
  await db.upsertAll('timemachine_numbers', rows);
  console.log(`[measure] wrote ${rows.length} rows to timemachine_numbers`);
}

main().catch(e => { console.error('[measure] FAILED', e); process.exit(1); });

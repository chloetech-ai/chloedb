// Policy Time Machine — API.
//
// A policy is a SQL predicate over recorded agent steps. Replay = one statement over the
// step tables. We read two honest numbers for any policy:
//   burden  — asks/denies per session over the benign history (timemachine_steps)
//   recall  — labelled attack traces where the policy fires at or before the harmful step
//             (timemachine_attack_steps), with n stated.
// Every number comes from a ChloeDB row; every panel ships its SQL to the drawer.
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const meta = {
  title: 'Policy Time Machine',
  tagline: 'Replay an agent-governance policy over recorded history: attacks stopped, honest work interrupted.',
  order: 2,
  // ids the hub card shows, from GET /timemachine/api/numbers, most important first.
  hub: [
    { id: 'combo_recall', label: 'labelled attacks a provenance+egress policy stops', format: 'pct' },
    { id: 'combo_burden', label: 'honest-work interruptions per session, same policy', format: 'n' },
    { id: 'replay_p50_ms', label: 'to replay a rule over every step', format: 'ms' },
  ],
};

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BENIGN = 'timemachine_steps';
const ATTACK = 'timemachine_attack_steps';

// ---- predicate safety: a policy is a boolean over known columns, nothing else ----------
const COLUMNS = new Set([
  'kind', 'tool', 'cmd', 'edit_op', 'path', 'dataset', 'repo', 'resolved', 'seq',
  'is_pkg_install', 'is_delete', 'is_egress', 'is_chmod', 'is_test_edit', 'is_write_src',
  'is_outside_repo', 'tainted', 'from_user', 'tainted_from', 'taint_snip', 'harm_step', 'is_harm',
]);
const KEYWORDS = new Set(['AND', 'OR', 'NOT', 'LIKE', 'ILIKE', 'IS', 'NULL', 'IN', 'TRUE', 'FALSE']);
// Returns { ok, error } — rejects anything that is not a plain predicate over COLUMNS.
export function validatePredicate(src) {
  if (!src || !src.trim()) return { ok: false, error: 'empty policy' };
  if (src.length > 600) return { ok: false, error: 'policy too long' };
  if (/;|--|\/\*|\bselect\b|\bdrop\b|\binsert\b|\bupdate\b|\bdelete\b|\bcreate\b|\balter\b|\bunion\b|\breachable\b|\bhops\b/i.test(src))
    return { ok: false, error: 'only a boolean condition over the columns is allowed (no subqueries, statements or comments)' };
  const re = /\s+|('(?:[^']|'')*')|(\d+(?:\.\d+)?)|([A-Za-z_][A-Za-z0-9_]*)|(<=|>=|!=|<>|=|<|>|\(|\)|%|,)/g;
  let m, i = 0;
  while ((m = re.exec(src))) {
    if (m.index !== i) return { ok: false, error: `unexpected character at ${i}: ${src.slice(i, m.index + 1)}` };
    i = re.lastIndex;
    const [, str, num, ident, op] = m;
    if (str || num || op) continue;
    if (ident) {
      const up = ident.toUpperCase();
      if (KEYWORDS.has(up)) continue;
      if (COLUMNS.has(ident.toLowerCase())) continue;
      return { ok: false, error: `unknown column or keyword: ${ident}` };
    }
  }
  if (i !== src.length) return { ok: false, error: `unexpected character at ${i}` };
  return { ok: true };
}

const qrec = (r, params) => ({ sql: r.sql, params: params || {}, took_ms: r.took_ms, rows: r.rows.length, plan: r.plan });

// ---- cached constants ------------------------------------------------------------------
const cache = { attackN: null, benignTotals: null, metaJson: null, frontier: null, wall: null };

async function benignTotals(db) {
  if (cache.benignTotals) return cache.benignTotals;
  const r = await db.query(`SELECT COUNT(*) AS steps, COUNT(DISTINCT session) AS sessions FROM ${BENIGN}`);
  cache.benignTotals = { steps: r.rows[0].steps, sessions: r.rows[0].sessions };
  return cache.benignTotals;
}
async function attackN(db) {
  if (cache.attackN) return cache.attackN;
  const r = await db.query(`SELECT label, COUNT(DISTINCT session) AS n FROM ${ATTACK} GROUP BY label`);
  const by = {}; let total = 0;
  for (const row of r.rows) { by[row.label] = row.n; total += row.n; }
  cache.attackN = { by, total };
  return cache.attackN;
}

// ---- replay: burden + recall + fired cells for one predicate ---------------------------
async function replay(db, predicate, action, { withCells = true } = {}) {
  const queries = [];
  const totals = await benignTotals(db);
  const N = await attackN(db);

  // burden over the benign history
  const bq = await db.query(`SELECT COUNT(*) AS fires, COUNT(DISTINCT session) AS sessions FROM ${BENIGN} WHERE ${predicate}`);
  queries.push(qrec(bq));
  const fires = bq.rows[0].fires, hitSessions = bq.rows[0].sessions;

  // recall over the labelled attack set (fires at or before the harmful step)
  const rq = await db.query(`SELECT label, COUNT(DISTINCT session) AS k FROM ${ATTACK} WHERE (${predicate}) AND seq <= harm_step GROUP BY label`);
  queries.push(qrec(rq));
  const recall = Object.keys(N.by).sort().map(label => ({
    label, k: (rq.rows.find(r => r.label === label)?.k) || 0, n: N.by[label],
  }));
  const recallK = recall.reduce((a, b) => a + b.k, 0);

  let fired = null, took = bq.took_ms, truncated = false;
  if (withCells) {
    const fq = await db.query(`SELECT session_idx, seq FROM ${BENIGN} WHERE ${predicate} LIMIT 9000`);
    queries.push(qrec(fq));
    fired = fq.rows.map(r => r.session_idx * 128 + r.seq);
    truncated = fired.length >= 9000 && fires > 9000;
    took = fq.took_ms;
  }
  return {
    action,
    burden: {
      fires, hit_sessions: hitSessions, sessions: totals.sessions, steps: totals.steps,
      per_session: +(fires / totals.sessions).toFixed(3),
      per_100: +(100 * fires / totals.steps).toFixed(3),
    },
    recall, recall_total: { k: recallK, n: N.total },
    fired, fired_truncated: truncated, took_ms: took, queries,
  };
}

// ---- preset frontier -------------------------------------------------------------------
async function frontier(db) {
  if (cache.frontier) return cache.frontier;
  const rr = await db.query(`SELECT id, name, action, predicate, descr, frontier, ord FROM timemachine_rules ORDER BY ord`);
  const points = [];
  for (const rule of rr.rows) {
    const r = await replay(db, rule.predicate, rule.action, { withCells: false });
    points.push({
      id: rule.id, name: rule.name, action: rule.action, predicate: rule.predicate, descr: rule.descr,
      frontier: rule.frontier, per_session: r.burden.per_session, per_100: r.burden.per_100,
      fires: r.burden.fires, recall: r.recall, recall_k: r.recall_total.k, recall_n: r.recall_total.n,
    });
  }
  cache.frontier = { points, query: qrec(rr) };
  return cache.frontier;
}

export async function init(ctx) {
  // warm the caches so the first page view is instant
  try { await benignTotals(ctx.db); await attackN(ctx.db); } catch (e) { console.error('[timemachine] init:', e.message); }
}

export async function handle(ctx) {
  const { db, json, res, route } = ctx;

  if (route === '/wall') {
    if (cache.wall) return json(res, 200, cache.wall);
    const sq = await db.query(`SELECT session_idx, session, repo, resolved, COUNT(*) AS n FROM ${BENIGN} GROUP BY session_idx, session, repo, resolved ORDER BY session_idx`);
    const lanes = sq.rows.map(r => ({ idx: r.session_idx, session: r.session, repo: r.repo, resolved: r.resolved, n: r.n }));
    // the step grid is larger than one query may return (10k cap) — page it by session_idx band.
    const codeOf = k => (k === 'command' ? 1 : k === 'edit' ? 2 : k === 'read' ? 3 : 0);
    const grid = {};
    let firstQ = null;
    for (let lo = 0; lo < lanes.length; lo += 120) {
      const kq = await db.query(`SELECT session_idx, seq, kind FROM ${BENIGN} WHERE session_idx >= :lo AND session_idx < :hi ORDER BY session_idx, seq LIMIT 9500`,
        { lo, hi: lo + 120 });
      if (!firstQ) firstQ = qrec(kq);
      for (const r of kq.rows) (grid[r.session_idx] || (grid[r.session_idx] = [])).push(codeOf(r.kind));
    }
    cache.wall = { lanes, grid, totals: await benignTotals(db), queries: [qrec(sq), firstQ] };
    return json(res, 200, cache.wall);
  }

  if (route === '/replay') {
    const body = await ctx.body();
    const predicate = (body?.predicate || '').trim();
    const action = body?.action === 'deny' ? 'deny' : 'ask';
    const v = validatePredicate(predicate);
    if (!v.ok) return json(res, 400, { error: v.error });
    try {
      const out = await replay(db, predicate, action, { withCells: body?.cells !== false });
      return json(res, 200, out);
    } catch (e) {
      return json(res, 400, { error: 'ChloeDB rejected the policy: ' + e.message });
    }
  }

  if (route === '/frontier') {
    const f = await frontier(db);
    return json(res, 200, { ...f, attack_n: await attackN(db) });
  }

  if (route === '/numbers') {
    const r = await db.query('SELECT id, value, n, method, model, measured_at, cost_usd, source FROM timemachine_numbers');
    const byId = {}; for (const row of r.rows) byId[row.id] = row;
    return json(res, 200, { numbers: byId, rows: r.rows, queries: [qrec(r)] });
  }

  if (route === '/step') {
    const id = ctx.query.get('id');
    const tbl = ctx.query.get('attack') === '1' ? ATTACK : BENIGN;
    if (!id || !/^[a-z0-9:]+$/i.test(id)) return json(res, 400, { error: 'bad id' });
    const r = await db.query(`SELECT * FROM ${tbl} WHERE id = :id`, { id });
    if (!r.rows.length) return json(res, 404, { error: 'no such step' });
    const step = r.rows[0];
    // the taint path: earlier steps this one is built from
    let edges = [];
    try { edges = JSON.parse(step.tainted_by || '[]'); } catch {}
    let parents = { rows: [] };
    if (edges.length) {
      const ids = edges.slice(0, 12).map(e => `'${String(e).replace(/'/g, "")}'`).join(',');
      parents = await db.query(`SELECT id, seq, kind, tool, cmd, tainted_from FROM ${tbl} WHERE id IN (${ids}) ORDER BY seq`);
    }
    return json(res, 200, { step, parents: parents.rows, queries: [qrec(r), ...(edges.length ? [qrec(parents)] : [])] });
  }

  if (route === '/taintpath') {
    // REACHABLE demo: benign steps reachable (against the edges) from an untrusted source.
    const src = ctx.query.get('src') || 'web';
    const seedWhere = src === 'web' ? "tainted_from LIKE '%web%'" : "from_user = 1";
    const r = await db.query(
      `SELECT id, session, seq, cmd FROM ${BENIGN}
       WHERE REACHABLE(tainted_by, (SELECT id FROM ${BENIGN} WHERE ${seedWhere} LIMIT 200), dir => 'in')
       LIMIT 200`);
    return json(res, 200, { rows: r.rows, queries: [qrec(r)] });
  }

  if (route === '/meta') {
    if (!cache.metaJson) cache.metaJson = JSON.parse(await readFile(path.join(HERE, 'data', 'meta.json'), 'utf8'));
    return json(res, 200, cache.metaJson);
  }

  if (route === '/translate') {
    const body = await ctx.body();
    const text = (body?.text || '').trim().slice(0, 400);
    if (!text) return json(res, 400, { error: 'say the rule in English' });
    const system = `You translate an English agent-governance rule into ONE ChloeDB SQL boolean predicate over recorded agent steps. Output ONLY the predicate (a WHERE condition), no statement, no semicolon, no SELECT.
Columns: kind ('command'|'edit'|'read'|'other'), tool, cmd (TEXT of the command / tool args), path, is_pkg_install, is_delete, is_egress, is_chmod, is_test_edit, is_write_src, is_outside_repo, tainted (built from earlier tool output), from_user, tainted_from (TEXT listing source classes: web mcp file test_output shell user), seq, is_harm (all 0/1 int flags).
Prefer the flag columns. Use LIKE '%...%' on cmd or tainted_from. Example: "block anything built from the web" -> is_egress = 1 OR tainted_from LIKE '%web%'.`;
    try {
      const resp = await ctx.llm.complete({ app: 'timemachine', ip: ctx.ip, model: 'claude-sonnet-5-5', max_tokens: 200, effort: 'low',
        system, messages: [{ role: 'user', content: text }] });
      let pred = ctx.llm.textOf(resp).trim().replace(/^```[a-z]*|```$/g, '').replace(/;+\s*$/, '').trim();
      const v = validatePredicate(pred);
      return json(res, 200, { predicate: pred, valid: v.ok, error: v.error || null, model: resp.model, cost_usd: resp.cost_usd });
    } catch (e) {
      return json(res, 503, { error: e.message, code: e.code });
    }
  }

  return false;
}

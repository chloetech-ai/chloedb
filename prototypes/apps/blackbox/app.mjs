// Black Box: the flight recorder for coding agents. The API behind public/index.html.
//
// Everything the page shows is read from the blackbox_* tables of ChloeDB (load.mjs loads them,
// measure.mjs writes the numbers); each answer carries the statements it ran as `queries`, so
// the page's drawer shows the SQL behind every panel. core.mjs holds what this file and
// measure.mjs both run.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ensureTables } from './load.mjs';
import * as core from './core.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

export const meta = {
  title: 'Black Box',
  tagline: 'The flight recorder for coding agents: one searchable, tamper-evident record across harness formats, where any line of a patch leads back to the step that wrote it.',
  order: 3,
  hub: [
    { id: 'blame_coverage', label: 'patch lines traced to the call that wrote them', format: 'pct' },
    { id: 'chain_verify_ms', label: 'to verify every session', format: 'ms' },
    { id: 'sessions', label: 'sessions on the record', format: 'n' },
  ],
};

export async function init() { await ensureTables(); }

const ASK_DAILY_USD = Number(process.env.BLACKBOX_ASK_DAILY_USD || 4);
const QUESTIONS = ['What was wrong, and what did the agent change to fix it?', 'Which commands failed, and what did the agent do next?', 'Did the agent edit any test files, and why?'];
const q = core.q;
const clip = (s, n) => { s = String(s ?? ''); return s.length > n ? `${s.slice(0, n)}\n… (${s.length - n} more characters)` : s; };
const validId = s => /^[a-z]+-\d{1,4}$/.test(s || '');
const validStep = s => /^[a-z]+-\d{1,4}:\d{1,5}$/.test(s || '');
const readJson = f => { try { return JSON.parse(fs.readFileSync(path.join(here, 'data', f), 'utf8')); } catch { return null; } };

// ---- the patch as rows, each added line numbered as blackbox_blame numbers it -------------------
function patchRows(patch, max = 1600) {
  const rows = [];
  let inFile = false, hunk = 0, k = 0, oldNo = 0, newNo = 0;
  for (const raw of String(patch || '').split('\n')) {
    if (rows.length >= max) { rows.push({ t: 'more' }); break; }
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    if (line.startsWith('diff --git ')) { inFile = true; hunk = 0; rows.push({ t: 'file', text: (line.match(/ b\/(.+)$/) || [])[1] || line.slice(11) }); continue; }
    if (!inFile) continue;
    if (line.startsWith('+++ ') || line.startsWith('--- ') || (!hunk && !line.startsWith('@@'))) continue;
    const h = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/);
    if (h) { oldNo = Number(h[1]); newNo = Number(h[2]); hunk++; rows.push({ t: 'hunk', text: line }); continue; }
    if (line.startsWith('+')) { rows.push({ t: 'add', n: newNo, k, text: line.slice(1) }); k++; newNo++; }
    else if (line.startsWith('-')) { rows.push({ t: 'del', n: oldNo, text: line.slice(1) }); oldNo++; }
    else if (line.startsWith('\\')) continue;
    else { rows.push({ t: 'ctx', n: newNo, text: line.slice(1) }); oldNo++; newNo++; }
  }
  return rows;
}

// ---- the session the Blame screen opens on, chosen by a rule, from the rows ---------------------
// A resolved OpenHands session with a short patch whose every line is traced, and in it the edit
// that wrote the most lines among those with a view, a thought and a failing output behind them.
let featured = null;
async function pickFeatured(db) {
  if (featured) return featured;
  const queries = [];
  const sql = `SELECT id, patch_lines, steps FROM blackbox_sessions
WHERE source = 'nebius-SWE-rebench-openhands-trajectories' AND resolved = 1
  AND patch_lines BETWEEN 8 AND 40 AND traced = patch_lines AND steps < 160 ORDER BY id LIMIT 200`;
  const c = await db.query(sql);
  queries.push(q(c));
  if (!c.rows.length) return { session: null, step: null, queries };
  const bsql = `SELECT session, step, count(*) AS n FROM blackbox_blame
WHERE via = 'edit' AND back >= 0 AND session IN (:sessions) GROUP BY session, step LIMIT 5000`;
  const b = await db.query(bsql, { sessions: c.rows.map(x => x.id) });
  queries.push({ ...q(b), params: { sessions: `${c.rows.length} sessions` } });
  const st = await db.query('SELECT id, links FROM blackbox_steps WHERE id IN (:ids) LIMIT 5000', { ids: b.rows.map(x => x.step) });
  queries.push({ ...q(st), params: { ids: `${b.rows.length} edit steps` } });
  const links = new Map(st.rows.map(x => [x.id, typeof x.links === 'string' ? JSON.parse(x.links) : (x.links || [])]));
  const score = x => { const l = links.get(x.step) || []; const has = k => l.some(e => e[0] === k); return (has('view') ? 1 : 0) + (has('thought') ? 1 : 0) + (has('failed') ? 2 : 0); };
  const best = b.rows.map(x => ({ ...x, score: score(x) })).sort((x, y) => y.score - x.score || y.n - x.n || (x.step < y.step ? -1 : 1))[0];
  featured = { session: best.session, step: best.step, rule: 'a resolved OpenHands session with a patch of 8 to 40 lines, every line traced; in it, the edit that wrote the most lines among those with a view, a thought and a failing output behind them', queries };
  return featured;
}

// ---- verifying everything: one run at a time, its result kept for a minute ----------------------
let verifyRun = null, verifyDone = null;
function verifyEverything(db, onProgress) {
  if (verifyDone && Date.now() - verifyDone.at < 60_000) return Promise.resolve({ ...verifyDone.result, cached_s: Math.round((Date.now() - verifyDone.at) / 1000) });
  if (!verifyRun) {
    const listeners = new Set();
    verifyRun = { listeners, promise: core.verifyAll(db, p => { for (const f of listeners) f(p); }).then(r => { verifyDone = { at: Date.now(), result: r }; return r; }).finally(() => { verifyRun = null; }) };
  }
  if (onProgress) verifyRun.listeners.add(onProgress);
  return verifyRun.promise;
}

let tamperBusy = false;

export async function handle(ctx) {
  const { db, route, res } = ctx;
  const ok = body => ctx.json(res, 200, body);
  const bad = (status, error) => ctx.json(res, status, { error, queries: [] });

  // Every measured number, by id. The page and the hub's card read their numbers from here.
  if (route === '/numbers') {
    const r = await db.query('SELECT id, value, n, k, unit, label, method, model, measured_at, cost_usd, source, sql_text FROM blackbox_numbers LIMIT 2000');
    return ok({ numbers: Object.fromEntries(r.rows.map(x => [x.id, x])), queries: [q(r)] });
  }

  // What is on the record, by source; the chain's root; the session Blame opens on; the model's spend.
  if (route === '/overview') {
    const sql = `SELECT source, harness, format, licence, count(*) AS sessions, sum(steps) AS steps, sum(bytes) AS bytes, sum(dropped) AS dropped
FROM blackbox_sessions GROUP BY source, harness, format, licence ORDER BY sessions DESC`;
    const r = await db.query(sql);
    const f = await pickFeatured(db);
    const what = Object.fromEntries((readJson('sources.json') || []).map(s => [s.source, s.what]));
    let spend = null;
    try { const s = await ctx.llm.spendStatus(); spend = { live: s.live && (s.by_app?.blackbox?.usd || 0) < ASK_DAILY_USD, today_usd: s.by_app?.blackbox?.usd || 0, cap_usd: ASK_DAILY_USD }; } catch {}
    return ok({
      sources: r.rows.map(x => ({ ...x, what: what[x.source] || '' })), featured: { session: f.session, step: f.step, rule: f.rule },
      root: core.recordedRoot(), canned: core.CANNED.map(({ id, label, mode, sql, note, params }) => ({ id, label, mode, sql, note, params })),
      questions: QUESTIONS, ask: { model: core.ASK_MODEL, ...spend }, queries: [q(r), ...f.queries],
    });
  }

  // ---- Ask across everything --------------------------------------------------------------------
  if (route.startsWith('/canned/')) {
    const r = await core.runCanned(db, route.slice(8));
    return r ? ok(r) : bad(404, 'no such question');
  }
  if (route === '/ask' && ctx.method === 'POST') {
    const body = await ctx.body();
    const text = String(body?.q || '').trim();
    if (!text) return bad(400, 'ask something');
    try { return ok(await core.search(db, text)); }
    catch (e) { return ctx.json(res, 200, { mode: /^(select|with)\b/i.test(text) ? 'sql' : 'meaning', error: e.message.replace(/^chloedb POST \S+ -> \d+: /, ''), lanes: [], queries: [] }); }
  }

  // ---- sessions ---------------------------------------------------------------------------------
  if (route === '/sessions') {
    const sql = 'SELECT id, title, harness, source, repo, steps, resolved, feedback, patch_lines, traced FROM blackbox_sessions ORDER BY id LIMIT 2000';
    const r = await db.query(sql);
    return ok({ sessions: r.rows, queries: [q(r)] });
  }
  let m = route.match(/^\/session\/([a-z0-9-]+)$/);
  if (m) {
    if (!validId(m[1])) return bad(400, 'no such session');
    const b = await core.blameOf(db, m[1]);
    if (!b) return bad(404, 'no such session');
    const sql = 'SELECT seq, role, kind, tool, path, err, exit_code, out_bytes FROM blackbox_steps WHERE session = :s ORDER BY seq LIMIT 10000';
    const r = await db.query(sql, { s: m[1] });
    const { patch, task, ...session } = b.session;
    const by = new Map(b.lines.map(l => [l.k, l]));
    return ok({
      session: { ...session, task: clip(task, 2400) }, strip: r.rows,
      patch: patchRows(patch).map(row => (row.t === 'add' ? { ...row, file: by.get(row.k)?.file || '', via: by.get(row.k)?.via || 'none', step: by.get(row.k)?.step || '', origin: by.get(row.k)?.origin || '', back: by.get(row.k)?.back ?? -1 } : row)),
      queries: [...b.queries, q(r, { s: m[1] })],
    });
  }
  // A window of steps with their text, for the scrubber.
  m = route.match(/^\/session\/([a-z0-9-]+)\/steps$/);
  if (m) {
    if (!validId(m[1])) return bad(400, 'no such session');
    const from = Math.max(0, Number(ctx.query.get('from')) || 0), to = Math.min(from + 40, Number(ctx.query.get('to')) || from + 12);
    const sql = 'SELECT id, seq, role, kind, tool, cmd, path, text, err, exit_code, out_bytes, raw_sha, hash, prev_hash FROM blackbox_steps WHERE session = :s AND seq BETWEEN :a AND :b ORDER BY seq LIMIT 60';
    const p = { s: m[1], a: from, b: to };
    const r = await db.query(sql, p);
    return ok({ steps: r.rows.map(x => ({ ...x, text: clip(x.text, 6000), cmd: clip(x.cmd, 1200) })), queries: [q(r, p)] });
  }

  // ---- blame ------------------------------------------------------------------------------------
  if (route === '/thread') {
    const step = ctx.query.get('step');
    if (!validStep(step)) return bad(400, 'no such step');
    const t = await core.thread(db, step);
    return t.edit ? ok(t) : bad(404, 'no such step');
  }

  // ---- Ask this session: a packed context, its receipt, and an answer over it -------------------
  if (route === '/ask/session' && ctx.method === 'POST') {
    const body = await ctx.body();
    const session = String(body?.session || ''), question = String(body?.question || '').trim().slice(0, 300);
    if (!validId(session)) return bad(400, 'no such session');
    if (question.length < 4) return bad(400, 'ask a question');
    const p = await core.pack(db, { session, question });
    const out = { session, question, receipt: p.receipt, tally: p.tally, tokens: p.tokens, pack_ms: p.took_ms, queries: [p.query] };
    // An answer already recorded for this very question costs nothing.
    const id = `${session}|${question}`;
    const have = await db.query('SELECT answer, model, cost_usd, asked_at FROM blackbox_answers WHERE id = :id', { id });
    out.queries.push(q(have, { id }));
    if (have.rows.length) return ok({ ...out, answer: have.rows[0].answer, model: have.rows[0].model, cost_usd: have.rows[0].cost_usd, recorded: true, asked_at: have.rows[0].asked_at, why: 'this question was already asked of this session; the recorded answer is shown' });
    let why = '';
    try {
      const s = await ctx.llm.spendStatus();
      if ((s.by_app?.blackbox?.usd || 0) >= ASK_DAILY_USD) throw Object.assign(new Error(`today's budget for this panel ($${ASK_DAILY_USD}) is spent`), { code: 'budget' });
      if (!p.context) throw Object.assign(new Error('the pack is empty'), { code: 'empty' });
      const a = await core.answer(ctx.llm, { question, context: p.context, ip: ctx.ip });
      // The three standing questions are kept, one row per session and question, so a second visitor pays nothing.
      if (QUESTIONS.includes(question) && a.text && !a.refused) {
        await db.write([{ op: 'upsert', table: 'blackbox_answers', doc: { id, session, question, answer: a.text, receipt: JSON.stringify(p.receipt), model: a.model, cost_usd: a.cost_usd, asked_at: new Date().toISOString(), sql_text: p.query.sql, took_ms: p.took_ms } }]).catch(() => {});
      }
      return ok({ ...out, answer: a.text, model: a.model, cost_usd: a.cost_usd, recorded: false });
    } catch (e) { why = e.code === 'budget' || e.code === 'rate' || e.code === 'no_key' ? e.message : 'the live model did not answer'; }
    // Past the cap, or without a key: the nearest recorded answer for this session, said to be recorded.
    const near = await db.query('SELECT question, answer, model, cost_usd, asked_at FROM blackbox_answers WHERE session = :s ORDER BY bm25(question, :q) DESC LIMIT 1', { s: session, q: question }).catch(() => ({ rows: [] }));
    if (near.rows?.length) { out.queries.push(q(near, { s: session, q: question })); return ok({ ...out, answer: near.rows[0].answer, model: near.rows[0].model, cost_usd: near.rows[0].cost_usd, recorded: true, recorded_question: near.rows[0].question, asked_at: near.rows[0].asked_at, why }); }
    return ok({ ...out, answer: null, recorded: true, why: `${why}; no answer is recorded for this session. The receipt below is live: it is what the model would have read.` });
  }
  if (route === '/answers') {
    const r = await db.query('SELECT session, question, model, cost_usd, asked_at FROM blackbox_answers ORDER BY session LIMIT 200');
    return ok({ answers: r.rows, queries: [q(r)] });
  }

  // ---- the ingest report: the canary sweep ------------------------------------------------------
  if (route === '/sweep') {
    const queries = [];
    const run = async (sql, params) => { const r = await db.query(sql, params || {}); queries.push(q(r, params)); return r.rows; };
    const cells = await run('SELECT redactor, origin, carrier, sum(planted) AS planted, sum(caught) AS caught, sum(partial) AS partial, sum(flagged) AS flagged FROM blackbox_sweep GROUP BY redactor, origin, carrier');
    const families = await run('SELECT redactor, origin, family, sum(planted) AS planted, sum(caught) AS caught FROM blackbox_sweep GROUP BY redactor, origin, family ORDER BY family LIMIT 5000');
    const flags = await run('SELECT redactor, rule, steps, flags, sample_step, sample_masked FROM blackbox_sweep_flags ORDER BY steps DESC LIMIT 40');
    // Planted documents to look at: one per carrier, of a generated secret (nothing here was ever a credential).
    const seen = new Set();
    const samples = (await run("SELECT id, carrier, family, host_step, doc, chloectx, nodwyn, gitleaks, noseyparker FROM blackbox_canaries WHERE origin = 'unshaped' AND family = 'base62-24' ORDER BY id LIMIT 200")).filter(x => !seen.has(x.carrier) && seen.add(x.carrier));
    return ok({ cells, families, flags, samples, queries });
  }

  // ---- the chain --------------------------------------------------------------------------------
  if (route === '/chain/verify') {
    const s = ctx.query.get('session');
    if (!validId(s)) return bad(400, 'no such session');
    const head = await db.query('SELECT id, head_hash, steps FROM blackbox_sessions WHERE id = :s', { s });
    if (!head.rows.length) return bad(404, 'no such session');
    const v = await core.verifySession(db, s, { head: head.rows[0].head_hash });
    return ok({ ...v, recorded_head: head.rows[0].head_hash, queries: [q(head, { s }), v.query] });
  }
  if (route === '/chain/verify-all') {
    const stream = ctx.sse(res);
    try {
      const r = await verifyEverything(db, p => stream.send('progress', p));
      stream.send('done', { ...r, failures: r.failures.slice(0, 20) });
    } catch (e) { stream.send('failed', { error: e.message }); }
    return stream.end();
  }
  // Flip one character of one step on a scratch copy of a session, and verify the copy.
  if (route === '/chain/tamper' && ctx.method === 'POST') {
    const body = await ctx.body();
    const s = String(body?.session || '');
    if (!validId(s)) return bad(400, 'no such session');
    if (tamperBusy) return bad(429, 'another tamper test is running; try again in a moment');
    tamperBusy = true;
    const run = `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
    const queries = [];
    try {
      const sql = 'SELECT id, seq, role, kind, tool, cmd, path, text, aux, out_bytes, raw_sha, prev_hash, hash FROM blackbox_steps WHERE session = :s ORDER BY seq LIMIT 10000';
      const r = await db.query(sql, { s });
      queries.push(q(r, { s }));
      if (!r.rows.length) return bad(404, 'no such session');
      if (r.rows.length > 800) return bad(400, 'pick a session of at most 800 steps');
      const strip = o => Object.fromEntries(Object.entries(o).filter(([, v]) => v != null && v !== ''));
      const docs = r.rows.map(x => strip({ ...x, id: `${run}:${x.seq}`, run, session: s, made_at: new Date().toISOString() }));
      await db.upsertAll('blackbox_scratch', docs, { size: 200 });
      const opts = { table: 'blackbox_scratch', where: 'run = :r', params: { r: run } };
      const before = await core.verifySession(db, s, opts);
      // The step to change: an output or a thought near the middle with some text in it.
      const mid = Math.floor(r.rows.length / 2);
      const target = [...r.rows].sort((a, b) => Math.abs(a.seq - mid) - Math.abs(b.seq - mid)).find(x => ['output', 'thought'].includes(x.kind) && /[a-z0-9]/i.test(x.text || '')) || r.rows.find(x => /[a-z0-9]/i.test(x.text || ''));
      if (!target) return bad(400, 'this session has no text to change');
      const text = target.text;
      let at = Math.floor(text.length / 2);
      while (at < text.length && !/[a-z0-9]/i.test(text[at])) at++;
      if (at >= text.length) at = text.search(/[a-z0-9]/i);
      const from = text[at];
      const to = /[0-9]/.test(from) ? String((Number(from) + 1) % 10) : from === from.toLowerCase() ? from.toUpperCase() : from.toLowerCase();
      const changed = text.slice(0, at) + to + text.slice(at + 1);
      const usql = 'UPDATE blackbox_scratch SET text = :t WHERE id = :id';
      const u = await db.sql(usql, { t: changed, id: `${run}:${target.seq}` });
      queries.push({ sql: usql, params: { id: `${run}:${target.seq}`, t: `the step's text with character ${at} changed` }, took_ms: u.took_ms, rows: 1 });
      const after = await core.verifySession(db, s, opts);
      queries.push(after.query);
      return ok({
        session: s, steps: r.rows.length, before: { ok: before.ok, steps: before.steps },
        changed: { seq: target.seq, kind: target.kind, at, from, to, around: [text.slice(Math.max(0, at - 36), at), text.slice(at + 1, at + 37)] },
        after: { ok: after.ok, bad: after.bad, ms: after.read_ms + after.hash_ms }, queries,
      });
    } finally {
      tamperBusy = false;
      // The scratch copy is removed whatever happened.
      db.query('SELECT id FROM blackbox_scratch WHERE run = :r LIMIT 10000', { r: run })
        .then(async x => { for (let i = 0; i < x.rows.length; i += 500) await db.write(x.rows.slice(i, i + 500).map(d => ({ op: 'delete', table: 'blackbox_scratch', key: d.id }))); })
        .catch(() => {});
    }
  }

  return false;
}

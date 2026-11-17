// Leak Lab, step 1: read the datasets and run the deterministic measurements, and write
// compact artifacts under data/ that load.mjs loads into any ChloeDB.
//
//   node apps/leaklab/prep.mjs                 everything
//   node apps/leaklab/prep.mjs matrix          only the compromised-agent matrix
//   node apps/leaklab/prep.mjs corpus|suite|samples|parity
//
// What it writes:
//   data/attacks.jsonl.gz   the attack corpus: every attack text on disk, with its source
//   data/sources.json       each source: file, rows, licence and where the licence is stated
//   data/matrix.json.gz     the 24 x 4 matrix: runs and every byte that left (the egress rows)
//   data/exfil_suite.json   Nodwyn's own exfiltration suite, as `go test -json` reported it
//   data/samples.json       a fixed sample of attack prompts and agent tasks for the live rounds;
//                           nothing reads it in this build (README.md, "Not built")
//   data/parity.json        the JavaScript port of binding.Lookup and L1 against the Go code
//
// Arm C needs data/leakgate, built here from harness/leakgate/main.go inside a COPY of the
// Nodwyn repository (NODWYN_SRC, default /home/user/fclaude/nodwyn; never modified).
// Parquet files are read through ChloeDB (POST /v1/import into scratch tables of this
// app's prefix, dropped afterwards), so CHLOEDB_URL must point at a dev server.
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';
import { CHANNELS } from './sim/channels.mjs';
import { Agent, ARMS, BINDINGS, CANARIES, compact, detect, lookup, l1Redact } from './sim/world.mjs';
import { LEAKGATE, rowsOf, runOps, runScenarios } from './sim/nodwyn.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(here, 'data');
const DS = process.env.LEAKLAB_DATASETS || '/home/user/datasets';
const NODWYN_SRC = process.env.NODWYN_SRC || '/home/user/fclaude/nodwyn';
const COPY = path.join(DATA, 'nodwyn-copy');
mkdirSync(DATA, { recursive: true });

const want = process.argv.slice(2);
const doing = name => !want.length || want.includes(name);
const sha = s => createHash('sha256').update(s).digest('hex');
const jsonl = file => readFileSync(file, 'utf8').split('\n').filter(Boolean).map(l => JSON.parse(l));
// A stable order that does not depend on the file's: by the hash of each item's text.
const stable = (items, key, salt = 'leaklab') => items.map(x => [sha(salt + key(x)), x]).sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(p => p[1]);

// ---- the Go harness ---------------------------------------------------------------------
function ensureLeakgate() {
  if (existsSync(LEAKGATE) && !process.env.LEAKLAB_REBUILD) return;
  if (!existsSync(path.join(COPY, 'go.mod'))) {
    console.log(`[prep] copying ${NODWYN_SRC} to data/nodwyn-copy (the original is not touched)`);
    cpSync(NODWYN_SRC, COPY, { recursive: true, filter: src => !src.includes(`${path.sep}.git${path.sep}`) && !src.endsWith(`${path.sep}.git`) });
  }
  mkdirSync(path.join(COPY, 'cmd', 'leakgate'), { recursive: true });
  cpSync(path.join(here, 'harness', 'leakgate', 'main.go'), path.join(COPY, 'cmd', 'leakgate', 'main.go'));
  console.log('[prep] go build ./cmd/leakgate');
  execFileSync('go', ['build', '-o', LEAKGATE, './cmd/leakgate'], { cwd: COPY, stdio: 'inherit', env: { ...process.env, CGO_ENABLED: '0' } });
}
function nodwynCommit() {
  try { return readFileSync(path.join(NODWYN_SRC, '.git', 'HEAD'), 'utf8').trim().replace(/^ref: /, '').length === 40
    ? readFileSync(path.join(NODWYN_SRC, '.git', 'HEAD'), 'utf8').trim()
    : readFileSync(path.join(NODWYN_SRC, '.git', readFileSync(path.join(NODWYN_SRC, '.git', 'HEAD'), 'utf8').trim().replace(/^ref: /, '')), 'utf8').trim();
  } catch { return 'unknown'; }
}

// ---- 1. the matrix ------------------------------------------------------------------------
function matrix() {
  ensureLeakgate();
  const runs = [], egress = [];
  const now = new Date().toISOString();
  const add = (run, rows, canary, extra = []) => {
    for (const r of rows) r.authorised = r.authorised ? 1 : 0;
    const d = detect(rows, canary.value, extra);
    run.leaked = d.leaked || run.forceLeak ? 1 : 0;
    delete run.forceLeak;
    run.exfil = d.exfil ? 1 : 0;
    run.sinks = d.sinks.join(' | ');
    run.bytes = rows.reduce((n, r) => n + Buffer.byteLength(r.raw || ''), 0);
    run.rows = rows.length;
    runs.push(run);
    for (const r of compact(rows, canary.value)) egress.push({ id: `${run.id}#${String(r.seq).padStart(3, '0')}`, run: run.id, round: 'matrix', defence: run.defence, channel: run.channel, canary: run.canary, ...r });
  };

  // Arms A, B1, B2: the simulated egress layer.
  for (const arm of ['A', 'B1', 'B2']) {
    for (const canary of Object.values(CANARIES)) {
      for (const ch of CHANNELS) {
        const agent = new Agent(arm, canary);
        ch.act(agent);
        const gate = agent.steps.find(s => s.gate && s.gate !== 'none')?.gate || (arm === 'A' ? 'none' : 'passed');
        add({
          id: `m.${arm}.${String(ch.n).padStart(2, '0')}.a.${canary.key[0]}.-`, round: 'matrix', defence: arm, channel: ch.n, channel_key: ch.key,
          attempt: 'a', attempt_note: ch.what, canary: canary.key, policy: '-', model: '', engine: 'simulated egress layer (sim/world.mjs)',
          decision: arm === 'A' ? 'no gate' : gate === 'substituted' ? 'placeholder replaced by the value' : 'placeholder left as it is',
          reason: arm === 'A' ? 'the agent\'s commands read the value from the environment' : gate === 'substituted' ? `the gate replaces the placeholder ${arm === 'B1' ? 'anywhere in' : 'in the headers of'} a request to a bound host` : 'nothing to replace: no placeholder where this gate looks, or the host is not bound',
          asked: 0, security_asks: 0, action: ch.act.toString().replace(/^a => /, ''), steps: JSON.stringify(agent.steps), ms: 0, cost_usd: 0, measured_at: now,
        }, agent.rows, canary);
      }
    }
  }

  // Arm C: Nodwyn's real session, once answering every confirm "no" and once "yes".
  for (const canary of Object.values(CANARIES)) {
    for (const policy of ['n', 'y']) {
      const scenarios = [];
      for (const ch of CHANNELS) for (const at of ch.scm) scenarios.push({ id: `${ch.n}.${at.id}`, scm: at.scm, mcp: at.mcp });
      const t0 = Date.now();
      const results = runScenarios(canary, policy, scenarios);
      console.log(`[prep] arm C, ${canary.key} canary, confirms answered "${policy}": ${results.length} sessions in ${Date.now() - t0} ms`);
      for (const res of results) {
        const [n, attempt] = res.id.split('.');
        const ch = CHANNELS.find(c => c.n === Number(n));
        const at = ch.scm.find(a => a.id === attempt);
        if (res.err) throw new Error(`leakgate ${res.id}: ${res.err}`);
        const prompts = res.prompts || [];
        let decision = res.decision;
        if (prompts.some(p => p.answer === 'n')) decision = prompts.every(p => p.security) ? 'confirm' : 'ask';
        const guard = [];
        for (const line of res.log.split('\n')) {
          if (!line) continue;
          let ev; try { ev = JSON.parse(line); } catch { continue; }
          if (ev.kind === 'egress.check') guard.push({ kind: 'egress.check', ...ev.payload });
          if (ev.kind === 'guard.decision' && ev.payload.origin === 'model' && ev.payload.prim !== 'scenario') guard.push({ kind: 'guard.decision', prim: ev.payload.prim, decision: ev.payload.decision, checker: ev.payload.checker || '', reason: ev.payload.reason || '' });
        }
        const reason = res.tool_error || prompts.map(p => p.reason).join('; ') || guard.filter(g => g.reason).map(g => g.reason).join('; ') || (res.tool_ok ? 'the call ran' : '');
        const rows = rowsOf(res);
        add({
          id: `m.C.${String(ch.n).padStart(2, '0')}.${attempt}.${canary.key[0]}.${policy}`, round: 'matrix', defence: 'C', channel: ch.n, channel_key: ch.key,
          attempt, attempt_note: at.note, canary: canary.key, policy, model: 'scripted (test/fakes/ollama)', engine: 'Nodwyn session (core.New) via harness/leakgate',
          decision, reason, asked: prompts.length, security_asks: prompts.filter(p => p.security).length,
          action: at.scm.replaceAll('{{NAME}}', canary.name) + (at.mcp ? '\n;; then the model calls forge__create_comment with what the tool returned' : ''),
          steps: JSON.stringify({ guard, prompts, http: (res.http || []).map(h => ({ host: h.host, method: h.method, path: h.path })) }),
          ms: res.ms, cost_usd: 0, measured_at: now,
          // An OAuth token of the fake provider anywhere at all means the over-granted token left.
          forceLeak: ch.oauth && res.oauth_token === 'seen',
        }, rows, canary);
      }
    }
  }
  const out = { made_at: now, nodwyn_commit: nodwynCommit(), bindings: BINDINGS, canaries: CANARIES, arms: ARMS,
    channels: CHANNELS.map(c => ({ n: c.n, key: c.key, group: c.group, title: c.title, what: c.what, approved: Boolean(c.approved), attempts: c.scm.length })), runs, egress };
  writeFileSync(path.join(DATA, 'matrix.json.gz'), gzipSync(JSON.stringify(out)));
  const leaks = {};
  for (const r of runs) { const k = r.defence; leaks[k] = leaks[k] || new Set(); if (r.leaked) leaks[k].add(r.channel + (r.policy === 'y' ? ' (after yes)' : '')); }
  for (const [k, v] of Object.entries(leaks)) console.log(`[prep] ${k}: leaking channels ${[...v].join(', ') || 'none'}`);
  console.log(`[prep] matrix: ${runs.length} runs, ${egress.length} egress rows -> data/matrix.json.gz`);
}

// ---- 2. Nodwyn's own exfiltration suite --------------------------------------------------------
function suite() {
  ensureLeakgate();
  console.log('[prep] go test ./test/exfil/... in the copy');
  const env = { ...process.env };
  for (const k of ['HTTPS_PROXY', 'HTTP_PROXY', 'https_proxy', 'http_proxy', 'ALL_PROXY', 'all_proxy']) delete env[k];
  const r = spawnSync('go', ['test', './test/exfil/...', '-json', '-count=1'], { cwd: COPY, encoding: 'utf8', maxBuffer: 1 << 28, env, timeout: 900000 });
  const events = r.stdout.split('\n').filter(Boolean).map(l => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
  const done = events.filter(e => e.Test && ['pass', 'fail', 'skip'].includes(e.Action));
  const tests = done.map(e => ({ name: e.Test, action: e.Action, elapsed_s: e.Elapsed ?? 0, top: !e.Test.includes('/'), scenario: e.Test.startsWith('TestScenarios/') }));
  const expects = {};
  for (const f of readdirSync(path.join(COPY, 'test', 'exfil', 'scenarios')).filter(f => f.endsWith('.scm'))) {
    const src = readFileSync(path.join(COPY, 'test', 'exfil', 'scenarios', f), 'utf8');
    expects[f.replace(/\.scm$/, '')] = { expect: (src.match(/^;; expect:\s*(\w+)/m) || [])[1] || '', why: src.split('\n').filter(l => l.startsWith(';;') && !l.includes('expect:')).map(l => l.replace(/^;;\s?/, '')).join(' ').slice(0, 400) };
  }
  for (const t of tests) if (t.scenario) Object.assign(t, expects[t.name.split('/')[1]] || {});
  const pkg = events.find(e => !e.Test && ['pass', 'fail'].includes(e.Action));
  const out = { ran_at: new Date().toISOString(), nodwyn_commit: nodwynCommit(), go: execFileSync('go', ['version'], { encoding: 'utf8' }).trim(), exit: r.status,
    package: pkg?.Package || '', package_action: pkg?.Action || '', package_elapsed_s: pkg?.Elapsed ?? null, tests };
  writeFileSync(path.join(DATA, 'exfil_suite.json'), JSON.stringify(out, null, 1));
  console.log(`[prep] exfil suite: ${tests.filter(t => t.top).length} tests, ${tests.filter(t => t.scenario).length} scenarios, ${tests.filter(t => t.action === 'fail').length} failed -> data/exfil_suite.json`);
}

// ---- 3. the JavaScript port against the Go code ----------------------------------------------
function parity() {
  ensureLeakgate();
  const ops = [];
  const hosts = ['api.forge.example', 'status.forge.example', 'collect.attacker.example', 'api.othermodel.example', 'forge.example', 'API.FORGE.EXAMPLE', ''];
  const methods = ['GET', 'POST', 'DELETE', 'PUT', 'post', ''];
  const positions = ['header:Authorization:Bearer', 'header:Authorization', 'header:X-Request-Tag', 'query:access_token', 'query:q', 'body', ''];
  const classes = [['forge-token', []], ['forge-oauth', ['repo:issues']], ['forge-oauth', ['repo:issues', 'admin:org']], ['other-key', []]];
  for (const [cls, scopes] of classes) for (const host of hosts) for (const method of methods) for (const position of positions) ops.push({ op: 'lookup', class: cls, host, method, position, scopes });
  const texts = [
    `token ${CANARIES.shaped.value} end`, `DB_PASSWORD=${CANARIES.unshaped.value}`, 'x-api-key: sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv', 'ghp_0123456789abcdefghijABCDEFGHIJ012345',
    'AKIAIOSFODNN7EXAMPLE and xoxb-1234567890-abcdefghij', 'nothing here', '{"Authorization":["Bearer ' + CANARIES.shaped.value + '"]}', 'sk-short', 'github_pat_11ABCDEFG0abcdefghijkl',
    'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U',
  ];
  for (const text of texts) ops.push({ op: 'redact', text });
  const got = runOps(ops);
  let same = 0; const diffs = [];
  ops.forEach((o, i) => {
    const g = got[i];
    if (o.op === 'lookup') {
      const j = lookup(o.class, o.host, o.method, o.position, o.scopes);
      if (j.bound === g.bound && j.reason === g.reason) same++; else diffs.push({ op: o, go: g, js: { bound: j.bound, reason: j.reason } });
    } else {
      const j = l1Redact(o.text);
      if (j.text === g.text) same++; else diffs.push({ op: o, go: g, js: j });
    }
  });
  writeFileSync(path.join(DATA, 'parity.json'), JSON.stringify({ made_at: new Date().toISOString(), n: ops.length, same, lookups: ops.filter(o => o.op === 'lookup').length, redactions: texts.length, diffs: diffs.slice(0, 20) }, null, 1));
  console.log(`[prep] port parity: ${same}/${ops.length} identical answers (${diffs.length} differ) -> data/parity.json`);
}

// ---- 4. the attack corpus ---------------------------------------------------------------------
async function parquet(file, table, columns, where = '') {
  const t = 'leaklab_tmp_' + table;
  await db.sql(`DROP TABLE IF EXISTS ${t}`).catch(() => {});
  const body = readFileSync(file);
  const res = await fetch(`${db.BASE}/v1/import?table=${t}&key=`, { method: 'POST', headers: { 'content-type': 'application/vnd.apache.parquet', ...(process.env.CHLOEDB_TOKEN ? { authorization: `Bearer ${process.env.CHLOEDB_TOKEN}` } : {}) }, body });
  if (!res.ok) throw new Error(`import ${file}: ${res.status} ${(await res.text()).slice(0, 300)}`);
  const out = [];
  // Read every row back with a scroll (a query's own window is 10,000 rows).
  let page = (await db.call('POST', '/v1/query', { sql: `SELECT ${columns.join(', ')} FROM ${t}${where ? ' WHERE ' + where : ''}`, objects: true, page_size: 5000 })).data;
  for (;;) {
    out.push(...page.rows);
    if (!page.cursor) break;
    page = (await db.call('POST', '/v1/query', { cursor: page.cursor, objects: true })).data;
  }
  await db.sql(`DROP TABLE IF EXISTS ${t}`);
  return out;
}

function csvRows(text) {
  const rows = []; let row = [], cell = '', inq = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (inq) { if (c === '"') { if (text[i + 1] === '"') { cell += '"'; i++; } else inq = false; } else cell += c; }
    else if (c === '"') inq = true;
    else if (c === ',') { row.push(cell); cell = ''; }
    else if (c === '\n') { row.push(cell); rows.push(row); row = []; cell = ''; }
    else if (c !== '\r') cell += c;
  }
  if (cell || row.length) { row.push(cell); rows.push(row); }
  const head = rows.shift();
  return rows.filter(r => r.length === head.length).map(r => Object.fromEntries(head.map((h, i) => [h, r[i]])));
}

const SOURCES = [
  { key: 'tensor-trust', name: 'Tensor Trust', kind: 'prompt extraction and hijacking attacks from the game', file: 'tensor-trust/{extraction,hijacking}_robustness_dataset.jsonl',
    licence: 'not stated in the files on disk', licence_from: 'tensor-trust/README.md names no licence; see github.com/HumanCompatibleAI/tensor-trust-data', url: 'https://github.com/HumanCompatibleAI/tensor-trust-data' },
  { key: 'mosscap', name: 'Lakera Mosscap', kind: 'password-extraction prompts from the game (test split)', file: 'lakera-mosscap/test.parquet',
    licence: 'MIT', licence_from: 'lakera-mosscap/README.md ("license: mit")', url: 'https://huggingface.co/datasets/Lakera/mosscap_prompt_injection' },
  { key: 'gandalf', name: 'Lakera Gandalf (ignore instructions)', kind: 'instruction-override prompts from the game', file: 'lakera-gandalf-ignore-instructions/train.parquet',
    licence: 'MIT (dataset card)', licence_from: 'the dataset card on Hugging Face; no licence file on disk', url: 'https://huggingface.co/datasets/Lakera/gandalf_ignore_instructions' },
  { key: 'deepset', name: 'deepset prompt-injections', kind: 'texts labelled as injections (label 1)', file: 'deepset-prompt-injections/{train,test}-*.parquet',
    licence: 'Apache-2.0 (dataset card)', licence_from: 'the dataset card on Hugging Face; no licence file on disk', url: 'https://huggingface.co/datasets/deepset/prompt-injections' },
  { key: 'bipia', name: 'BIPIA', kind: 'indirect-injection attack strings (distinct attack_str)', file: 'bipia/train.parquet',
    licence: 'CC BY-SA 4.0 (this redistribution); BIPIA code MIT', licence_from: 'bipia/README.md ("license: cc-by-sa-4.0", "BIPIA code is MIT")', url: 'https://github.com/microsoft/BIPIA' },
  { key: 'llmail', name: 'LLMail-Inject', kind: 'attack emails submitted to the challenge (phase 2, first 2,501)', file: 'llmail-inject-challenge/raw_submissions_phase2.head.jsonl',
    licence: 'MIT', licence_from: 'llmail-inject-challenge/README.md ("license: mit")', url: 'https://huggingface.co/datasets/microsoft/llmail-inject-challenge' },
  { key: 'injecagent', name: 'InjecAgent', kind: 'attacker instructions planted in tool results', file: 'InjecAgent/repo/data/test_cases_{ds,dh}_base.json',
    licence: 'MIT', licence_from: 'InjecAgent/repo/LICENCE', url: 'https://github.com/uiuc-kang-lab/InjecAgent' },
  { key: 'nemotron', name: 'Nemotron indirect prompt injection', kind: 'injections planted in an agent\'s tool data', file: 'nemotron-indirect-prompt-injection/train.jsonl',
    licence: 'CC BY 4.0', licence_from: 'nemotron-indirect-prompt-injection/README.md ("license: cc-by-4.0")', url: 'https://huggingface.co/datasets/nvidia/Nemotron-RL-Agentic-Indirect-Prompt-Injection-v1' },
  { key: 'b3', name: 'Lakera b3 (low-quality release)', kind: 'crowd-sourced attacks on agent backbones', file: 'lakera-b3/selected_attacks_low-quality.csv',
    licence: 'Lakera research-only terms (non-commercial, attribution, no redistribution outside research)', licence_from: 'lakera-b3/LICENSE', url: 'https://huggingface.co/datasets/Lakera/b3-agent-security-benchmark-weak' },
];

async function corpus() {
  const rows = [];
  const push = (source, kind, text, extra = '') => {
    text = String(text ?? '').replace(/\u0000/g, '').trim();
    if (text.length < 8) return;
    rows.push({ source, kind, text: text.slice(0, 2000), chars: text.length, extra });
  };
  const tt = f => jsonl(path.join(DS, 'tensor-trust', f));
  for (const r of tt('extraction_robustness_dataset.jsonl')) push('tensor-trust', 'extraction', r.attack);
  for (const r of tt('hijacking_robustness_dataset.jsonl')) push('tensor-trust', 'hijacking', r.attack);
  for (const r of await parquet(path.join(DS, 'lakera-mosscap', 'test.parquet'), 'mosscap', ['level', 'prompt'])) push('mosscap', 'extraction', r.prompt, r.level);
  for (const r of await parquet(path.join(DS, 'lakera-gandalf-ignore-instructions', 'train.parquet'), 'gandalf', ['text'])) push('gandalf', 'override', r.text);
  for (const f of readdirSync(path.join(DS, 'deepset-prompt-injections')).filter(f => f.endsWith('.parquet')).sort()) {
    for (const r of await parquet(path.join(DS, 'deepset-prompt-injections', f), 'deepset', ['text', 'label'])) if (Number(r.label) === 1) push('deepset', 'injection', r.text);
  }
  for (const r of await parquet(path.join(DS, 'bipia', 'train.parquet'), 'bipia', ['attack_str', 'attack_name', 'attack_category'])) push('bipia', r.attack_category || 'indirect', r.attack_str, r.attack_name);
  for (const r of jsonl(path.join(DS, 'llmail-inject-challenge', 'raw_submissions_phase2.head.jsonl'))) push('llmail', 'email', `${r.subject ? 'Subject: ' + r.subject + '\n' : ''}${r.body || ''}`, r.scenario || '');
  for (const f of ['test_cases_ds_base.json', 'test_cases_dh_base.json']) {
    for (const r of JSON.parse(readFileSync(path.join(DS, 'InjecAgent', 'repo', 'data', f), 'utf8'))) push('injecagent', f.includes('_ds_') ? 'data stealing' : 'direct harm', r['Attacker Instruction'], r['Attack Type']);
  }
  for (const r of jsonl(path.join(DS, 'nemotron-indirect-prompt-injection', 'train.jsonl'))) push('nemotron', r.attack_category, r.injection?.injection_text || r.injection?.goal, r.domain);
  if (!process.env.LEAKLAB_SKIP_B3) for (const r of csvRows(readFileSync(path.join(DS, 'lakera-b3', 'selected_attacks_low-quality.csv'), 'utf8'))) push('b3', r.app_slug, r.attack, `level ${r.level}`);

  // One row per distinct text of a source; the count of repeats is kept.
  const seen = new Map();
  for (const r of rows) {
    const k = r.source + '\u0001' + r.text;
    const had = seen.get(k);
    if (had) had.copies += 1; else seen.set(k, { ...r, copies: 1 });
  }
  const per = {}, raw = {};
  for (const r of rows) raw[r.source] = (raw[r.source] || 0) + 1;
  const out = [];
  for (const r of seen.values()) {
    per[r.source] = (per[r.source] || 0) + 1;
    out.push({ id: `${r.source}-${sha(r.text).slice(0, 12)}`, ...r });
  }
  writeFileSync(path.join(DATA, 'attacks.jsonl.gz'), gzipSync(out.map(r => JSON.stringify(r)).join('\n') + '\n'));
  const sources = SOURCES.filter(s => per[s.key]).map(s => ({ ...s, rows: per[s.key], rows_on_disk: raw[s.key] }));
  writeFileSync(path.join(DATA, 'sources.json'), JSON.stringify({ made_at: new Date().toISOString(), sources }, null, 1));
  console.log(`[prep] corpus: ${out.length} distinct texts from ${rows.length} rows -> data/attacks.jsonl.gz`);
  for (const s of sources) console.log(`        ${s.key.padEnd(13)} ${String(s.rows).padStart(6)} distinct of ${s.rows_on_disk}  (${s.licence})`);
  return out;
}

// ---- 5. what measure.mjs sends to a model -----------------------------------------------------
async function samples() {
  // Talk it out: 200 Tensor Trust extraction attacks and 200 Mosscap prompts, unmodified.
  const tt = stable(jsonl(path.join(DS, 'tensor-trust', 'extraction_robustness_dataset.jsonl')).filter(r => r.attack && r.attack.length >= 12 && r.attack.length <= 1500), r => r.attack).slice(0, 200)
    .map(r => ({ id: 'tt-' + r.sample_id, source: 'tensor-trust', text: r.attack }));
  const mossAll = await parquet(path.join(DS, 'lakera-mosscap', 'test.parquet'), 'mosscap', ['level', 'prompt']);
  const mossSeen = new Set();
  const moss = stable(mossAll.filter(r => r.prompt && r.prompt.length >= 20 && r.prompt.length <= 1500 && !mossSeen.has(r.prompt) && mossSeen.add(r.prompt)), r => r.prompt).slice(0, 200)
    .map(r => ({ id: 'mc-' + sha(r.prompt).slice(0, 10), source: 'mosscap', text: r.prompt, level: r.level }));

  // Follow rate: 30 InjecAgent data-stealing cases and 30 Nemotron exfiltration tasks, unmodified.
  const tools = {};
  for (const kit of JSON.parse(readFileSync(path.join(DS, 'InjecAgent', 'repo', 'data', 'tools.json'), 'utf8'))) {
    for (const t of kit.tools) tools[kit.toolkit + t.name] = { name: kit.toolkit + t.name, description: t.summary, parameters: t.parameters || [] };
  }
  const sim = JSON.parse(readFileSync(path.join(DS, 'InjecAgent', 'repo', 'data', 'attacker_simulated_responses.json'), 'utf8'));
  const ia = stable(JSON.parse(readFileSync(path.join(DS, 'InjecAgent', 'repo', 'data', 'test_cases_ds_base.json'), 'utf8')), r => r['Attacker Instruction'] + r['User Instruction']).slice(0, 30)
    .map((r, i) => ({
      id: 'ia-' + String(i + 1).padStart(2, '0'), source: 'injecagent', attack_type: r['Attack Type'],
      user: r['User Instruction'], user_tool: r['User Tool'], user_tool_params: r['Tool Parameters'], tool_response: r['Tool Response'],
      attacker_tools: r['Attacker Tools'], attacker_instruction: r['Attacker Instruction'],
      tools: [r['User Tool'], ...r['Attacker Tools']].map(n => tools[n]).filter(Boolean),
      simulated: Object.fromEntries(r['Attacker Tools'].map(t => [t, sim[`(${t},{})`] ?? Object.entries(sim).find(([k]) => k.startsWith(`(${t},`))?.[1] ?? '{"status":"ok"}'])),
    }));
  const nm = stable(jsonl(path.join(DS, 'nemotron-indirect-prompt-injection', 'train.jsonl')).filter(r => r.attack_category === 'exfiltration'), r => String(r.id)).slice(0, 30)
    .map(r => ({
      id: 'nm-' + r.id, source: 'nemotron', domain: r.domain, vector: r.injection_vector,
      system: r.responses_create_params.input.find(m => m.role === 'system')?.content || '', user: r.responses_create_params.input.find(m => m.role === 'user')?.content || '',
      tools: r.responses_create_params.tools.map(t => t.function || t).map(t => ({ name: t.name, description: t.description, input_schema: t.parameters || { type: 'object', properties: {} } })),
      environment: r.environment, injection: r.injection, required_tools: r.required_tools,
    }));
  writeFileSync(path.join(DATA, 'samples.json'), JSON.stringify({ made_at: new Date().toISOString(), talk: [...tt, ...moss], follow: [...ia, ...nm] }));
  console.log(`[prep] samples: ${tt.length} Tensor Trust + ${moss.length} Mosscap prompts, ${ia.length} InjecAgent + ${nm.length} Nemotron tasks -> data/samples.json`);
}

if (doing('matrix')) matrix();
if (doing('parity')) parity();
if (doing('suite')) suite();
if (doing('corpus')) await corpus();
if (doing('samples')) await samples();

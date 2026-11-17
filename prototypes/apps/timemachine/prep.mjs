// Policy Time Machine — prep.
//
// Reads the three recorded datasets *here* and writes compact artifacts under data/:
//   data/benign.jsonl   — benign history steps (OpenHands SWE-rebench, 300 sessions)
//   data/attacks.jsonl  — labelled attack steps (AgentDojo succeeded + MCPHunt unsafe_success)
//   data/rules.json     — the preset policies
//   data/meta.json      — counts, dataset licences, how taint was approximated
//
// A step = one agent tool call. For every step we precompute feature columns so a policy
// is a plain SQL predicate (is_pkg_install=1, is_egress=1, kind='command' AND tainted=1, ...),
// and an approximate taint: `tainted_by` edges to earlier outputs in the same session that
// share a verbatim substring of >= 20 characters with this step's arguments, with the source
// class of each such output (web, mcp, file, test output, user). This is taint approximated
// from the record by shared substrings — NOT Nodwyn's value-level taint.
//
// Parquet is read by importing it into a temp table on CHLOEDB_URL (ChloeDB reads Parquet);
// AgentDojo and MCPHunt are plain JSON on disk. Datasets are untrusted data: we only read.
import { readFile, writeFile, readdir, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(HERE, 'data');
const DS = '/home/user/datasets';
const PARQUET = `${DS}/nebius-SWE-rebench-openhands-trajectories/sample-300.parquet`;
const AGENTDOJO = `${DS}/agentdojo/repo/runs`;
const MCPHUNT = `${DS}/mcphunt-agent-traces/deepseek_v4_flash_baseline.json`;

const MINLEN = 20;            // shared-substring length for the taint approximation
const OUT_CAP = 6000;         // chars of each output used for taint
const ARG_CAP = 4000;         // chars of each step's args used for taint

// ---------- taint: shared verbatim substring of >= 20 chars ----------------------------
// Rolling set of 20-char windows seen in earlier outputs of the same session, each mapped to
// the step that first produced it and that step's source class. A window that is only
// whitespace/punctuation is ignored (trivial overlap).
function trivial(w) {
  let alnum = 0;
  for (const c of w) if (/[A-Za-z0-9]/.test(c)) alnum++;
  return alnum < 8;            // need a reasonable run of real characters
}
function windows(s) {
  const out = [];
  if (!s) return out;
  const t = s.slice(0, OUT_CAP);
  for (let i = 0; i + MINLEN <= t.length; i += 1) {
    const w = t.slice(i, i + MINLEN);
    if (!trivial(w)) out.push(w);
  }
  return out;
}

class Taint {
  constructor() { this.map = new Map(); }           // window -> {stepId, cls}
  addOutput(stepId, cls, text) {
    for (const w of windows(text)) if (!this.map.has(w)) this.map.set(w, { stepId, cls });
  }
  // For a step's args: find earlier outputs it shares a >=20 char window with.
  check(argText) {
    const hits = new Map();                          // stepId -> cls
    const classes = new Set();
    let snippet = '';
    for (const w of windows(argText.slice(0, ARG_CAP))) {
      const h = this.map.get(w);
      if (h) {
        if (!hits.has(h.stepId)) hits.set(h.stepId, h.cls);
        classes.add(h.cls);
        if (snippet.length < w.length) snippet = w;   // one representative shared run
      }
    }
    return { ids: [...hits.keys()], classes: [...classes].sort(), snippet };
  }
}

// ---------- feature classification (shared vocabulary across datasets) ------------------
const PKG = /\b(pip3?\s+install|python3?\s+-m\s+pip\s+install|conda\s+install|npm\s+(i|install)\b|yarn\s+add|pnpm\s+add|apt(-get)?\s+install|poetry\s+add|cargo\s+install|gem\s+install|go\s+get)\b/i;
const DEL = /(\brm\s+-[rf]|\brm\s+--|\brm\s+\/|\brm\s+[^|&;]*\b|\bunlink\b|\brmdir\b|shutil\.rmtree|os\.remove|\bfind\b[^|]*-delete)/i;
const DEL_STRICT = /(\brm\s+-|\brm\s+[^-]|\bunlink\b|\brmdir\b|rmtree|os\.remove)/i;
const EGRESS = /(\bcurl\b|\bwget\b|\bnc\b|\bncat\b|\bscp\b|\bsftp\b|\bssh\b|git\s+push|npm\s+publish|\brequests\.(get|post|put)|urllib|httpx|http\.client|telnet)/i;
const TESTISH = /(^|[\/_.-])(tests?|test_|_test|conftest|spec)([\/_.-]|$|\.)/i;
const SRC_READ = /^(cat|less|more|head|tail|ls|grep|rg|find|tree|git\s+diff|git\s+log|git\s+show|pwd|sed\s+-n|awk|wc|file|stat|which|echo)\b/i;
const TEST_RUN = /\b(pytest|py\.test|python3?\s+-m\s+pytest|tox|nox|unittest|npm\s+test|jest|mocha|go\s+test|cargo\s+test|make\s+test)\b/i;

function firstCmd(s) { return (s || '').trim().split(/[\n|&;]/)[0].trim(); }

// A benign OpenHands step -> normalized feature record.
function featBash(cmd) {
  const c = cmd || '';
  const head = firstCmd(c);
  return {
    kind: 'command',
    is_pkg_install: PKG.test(c) ? 1 : 0,
    is_delete: DEL_STRICT.test(c) ? 1 : 0,
    is_egress: EGRESS.test(c) ? 1 : 0,
    is_chmod: /\bchmod\b|\bchown\b/.test(c) ? 1 : 0,
    is_test_edit: 0,
    is_write_src: 0,
    is_outside_repo: 0,
    reads: SRC_READ.test(head) ? 1 : 0,
    runs_tests: TEST_RUN.test(c) ? 1 : 0,
  };
}
function featEdit(editCmd, p) {
  const writing = editCmd === 'create' || editCmd === 'str_replace' || editCmd === 'insert';
  const inWorkspace = /\/workspace\//.test(p || '');
  return {
    kind: writing ? 'edit' : 'read',
    is_pkg_install: 0, is_delete: 0, is_egress: 0, is_chmod: 0,
    is_test_edit: writing && TESTISH.test(p || '') ? 1 : 0,
    is_write_src: writing ? 1 : 0,
    is_outside_repo: writing && !inWorkspace ? 1 : 0,
    reads: writing ? 0 : 1,
    runs_tests: 0,
  };
}

function outClassBash(feat, head) {
  if (feat.is_egress) return 'web';
  if (feat.runs_tests) return 'test_output';
  if (feat.reads) return 'file';
  return 'shell';
}

// ---------- OpenHands (benign) ----------------------------------------------------------
function parseArgs(tc) { try { return JSON.parse(tc.function.arguments); } catch { return {}; } }

function normalizeOpenHands(row, sidx) {
  const sid = 'b' + String(sidx).padStart(3, '0');
  const traj = JSON.parse(row.trajectory);
  // map tool_call_id -> result text
  const results = new Map();
  for (const m of traj) if (m.role === 'tool' && m.tool_call_id) results.set(m.tool_call_id, m.content || '');
  const userMsg = (traj.find(m => m.role === 'user')?.content) || '';
  const taint = new Taint();
  taint.addOutput(sid + ':u', 'user', userMsg);     // the task prompt is a 'user' source
  const steps = [];
  let seq = 0;
  for (const m of traj) {
    if (m.role !== 'assistant') continue;
    for (const tc of (m.tool_calls || [])) {
      const fn = tc.function?.name;
      if (fn === 'think' || fn === 'finish' || fn === 'task_tracker') continue;
      const a = parseArgs(tc);
      let feat, cmdText, editCmd = '', p = '';
      if (fn === 'execute_bash') {
        cmdText = a.command || '';
        feat = featBash(cmdText);
      } else if (fn === 'str_replace_editor') {
        editCmd = a.command || '';
        p = a.path || '';
        feat = featEdit(editCmd, p);
        cmdText = `${editCmd} ${p}` + (a.new_str ? ' ' + a.new_str : '') + (a.file_text ? ' ' + a.file_text : '');
      } else {
        feat = { kind: 'other', is_pkg_install: 0, is_delete: 0, is_egress: 0, is_chmod: 0, is_test_edit: 0, is_write_src: 0, is_outside_repo: 0, reads: 0, runs_tests: 0 };
        cmdText = JSON.stringify(a).slice(0, 400);
      }
      const argText = fn === 'execute_bash' ? cmdText : JSON.stringify(a);
      const tt = taint.check(argText);
      const id = `${sid}:${String(seq).padStart(3, '0')}`;
      steps.push({
        id, dataset: 'openhands', session: sid, session_idx: sidx, seq,
        kind: feat.kind, tool: fn, cmd: (fn === 'execute_bash' ? cmdText : `${editCmd} ${p}`).slice(0, 300),
        edit_op: editCmd, path: (p || '').slice(0, 200),
        is_pkg_install: feat.is_pkg_install, is_delete: feat.is_delete, is_egress: feat.is_egress,
        is_chmod: feat.is_chmod, is_test_edit: feat.is_test_edit, is_write_src: feat.is_write_src,
        is_outside_repo: feat.is_outside_repo,
        tainted: tt.classes.some(c => c !== 'user') ? 1 : 0, from_user: tt.classes.includes('user') ? 1 : 0, tainted_from: tt.classes.join(' '),
        tainted_by: tt.ids, taint_snip: tt.snippet.slice(0, 120),
        label: '', attack: '', harm_step: -1, is_harm: 0,
        repo: row.repo || '', resolved: row.resolved | 0,
      });
      // record this step's own output for later steps' taint
      const out = results.get(tc.id) || '';
      const cls = fn === 'str_replace_editor' ? (feat.reads ? 'file' : 'file') : outClassBash(feat, firstCmd(cmdText));
      taint.addOutput(id, cls, out);
      seq++;
    }
  }
  return steps;
}

// ---------- AgentDojo (labelled: attacker goal reached) ---------------------------------
// security === true means the injected task's goal was achieved (verified by reading the
// tool calls: e.g. injection_task_5 fires invite_user_to_slack Fred fred9246@gmail.com only
// when security=true). We take those runs as the labelled attack set.
function collectInjectionTargets(injections) {
  // long verbatim fragments of the injected instruction — what a harmful action reuses.
  const frags = [];
  for (const v of Object.values(injections || {})) {
    const t = String(v);
    // explicit artifacts the attacker wants exfiltrated/used
    for (const m of t.matchAll(/\b(?:www\.|https?:\/\/)[^\s"'<)]+/g)) frags.push(m[0]);
    for (const m of t.matchAll(/[\w.+-]+@[\w.-]+\.\w+/g)) frags.push(m[0]);
  }
  return frags.filter(f => f.length >= 8);
}

async function* walkJson(dir) {
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) yield* walkJson(full);
    else if (e.name.endsWith('.json')) yield full;
  }
}

async function normalizeAgentDojo(sidxStart) {
  const steps = [];
  let sidx = sidxStart, n = 0, kept = 0;
  const files = [];
  for await (const f of walkJson(AGENTDOJO)) if (f.includes('important_instructions')) files.push(f);
  files.sort();
  for (const f of files) {
    const d = JSON.parse(await readFile(f, 'utf8'));
    if (d.security !== true) continue;                 // only runs where the attacker's goal was reached
    kept++;
    const sid = 'ad' + String(kept).padStart(2, '0');
    const attack = `${d.suite_name}/${d.user_task_id}+${d.injection_task_id}`;
    const targets = collectInjectionTargets(d.injections);
    const injectionText = Object.values(d.injections || {}).map(String).join('\n');
    const taint = new Taint();
    const userMsg = (d.messages.find(m => m.role === 'user')?.content) || '';
    taint.addOutput(sid + ':u', 'user', userMsg);
    const stepRows = [];
    let seq = 0, harm = -1;
    for (const m of d.messages) {
      if (m.role === 'tool') {
        // a tool result — its content may carry the injected instruction (web/channel/file)
        const content = String(m.content || '');
        const fn = m.tool_call?.function || '';
        const cls = /get_webpage|download|url|web/i.test(fn) ? 'web' : 'mcp';
        // output belongs to the assistant tool-call step that produced it; approximate by a synthetic id
        taint.addOutput(sid + ':o' + seq, content.includes(injectionText.slice(0, 40)) || targets.some(t => content.includes(t)) ? cls : cls, content);
        continue;
      }
      if (m.role !== 'assistant') continue;
      for (const tc of (m.tool_calls || [])) {
        const fn = tc.function;
        const a = tc.args || {};
        const argText = `${fn} ${JSON.stringify(a)}`;
        const tt = taint.check(argText);
        // is this the harmful step? it reuses an attacker artifact / injected fragment
        const usesTarget = targets.some(t => argText.includes(t));
        const OUT = /send|post|invite|share|transfer|pay|forward|upload|add_user/i.test(fn);  // genuine outbound / exfil
        const SEND = OUT || /create_|update_|write|delete|remove_user/i.test(fn);
        const isHarm = usesTarget && SEND ? 1 : 0;
        if (isHarm && harm < 0) harm = seq;
        const id = `${sid}:${String(seq).padStart(3, '0')}`;
        stepRows.push({
          id, dataset: 'agentdojo', session: sid, session_idx: sidx, seq,
          kind: 'command', tool: fn, cmd: `${fn} ${JSON.stringify(a)}`.slice(0, 300),
          edit_op: '', path: '',
          is_pkg_install: 0, is_delete: /delete|remove_user/i.test(fn) ? 1 : 0,
          is_egress: OUT ? 1 : 0,
          is_chmod: 0, is_test_edit: 0, is_write_src: /create_file|write/i.test(fn) ? 1 : 0, is_outside_repo: 0,
          tainted: tt.classes.some(c => c !== 'user') ? 1 : 0, from_user: tt.classes.includes('user') ? 1 : 0, tainted_from: tt.classes.join(' '),
          tainted_by: tt.ids, taint_snip: tt.snippet.slice(0, 120),
          label: 'agentdojo', attack, harm_step: -1, is_harm: isHarm,
          repo: d.suite_name || '', resolved: 0,
        });
        seq++;
      }
    }
    if (harm < 0) harm = Math.max(0, seq - 1);          // goal reached but artifact not matched: last action
    for (const s of stepRows) { s.harm_step = harm; if (s.seq === harm && !stepRows.some(x => x.is_harm)) s.is_harm = s.seq === harm ? 1 : 0; }
    steps.push(...stepRows);
    sidx++; n++;
  }
  return { steps, n, nextSidx: sidx };
}

// ---------- MCPHunt (labelled: unsafe_success, per-turn canary labels) ------------------
function mcpSourceClass(server) {
  if (/browser|web|http/i.test(server)) return 'web';
  if (/file/i.test(server)) return 'file';
  return 'mcp';
}

async function normalizeMcpHunt(sidxStart) {
  const raw = JSON.parse(await readFile(MCPHUNT, 'utf8'));
  const traces = (raw.traces || []).filter(t => t.outcome === 'unsafe_success');
  const steps = [];
  let sidx = sidxStart, kept = 0;
  for (const t of traces) {
    kept++;
    const sid = 'mh' + String(kept).padStart(2, '0');
    const ev = t.events || [];
    // harmful turn = first canary leak (min write_turn of taint_flows)
    const flows = (t.labeling?.taint_flows) || [];
    const harmTurn = flows.length ? Math.min(...flows.map(f => f.write_turn ?? 1e9)) : null;
    const taint = new Taint();
    taint.addOutput(sid + ':u', 'user', String(t.task_prompt || ''));
    const stepRows = [];
    let seq = 0, harm = -1;
    for (const e of ev) {
      const server = e.server || '';
      const tool = e.tool || '';
      const a = e.args || {};
      const cls = mcpSourceClass(server);
      const argText = `${server}.${tool} ${JSON.stringify(a)}`;
      const tt = taint.check(argText);
      const isWrite = /write|save|create|append|put|post|upload|send|push/i.test(tool);
      const isHarm = (harmTurn != null && e.turn === harmTurn && isWrite) ? 1 : 0;
      if (isHarm && harm < 0) harm = seq;
      const id = `${sid}:${String(seq).padStart(3, '0')}`;
      stepRows.push({
        id, dataset: 'mcphunt', session: sid, session_idx: sidx, seq,
        kind: 'command', tool: `${server}.${tool}`, cmd: `${server}.${tool} ${JSON.stringify(a)}`.slice(0, 300),
        edit_op: '', path: String(a.path || '').slice(0, 200),
        is_pkg_install: 0, is_delete: /delete|remove|unlink/i.test(tool) ? 1 : 0,
        // genuine outbound only: MCPHunt's browser_to_local exfiltrates to a LOCAL file,
        // so an egress rule must miss it (that is the provenance argument). Navigating/reading
        // a page is not egress.
        is_egress: /send_email|send_message|http_post|post_request|upload|publish|smtp|sendmail/i.test(tool) ? 1 : 0,
        is_chmod: 0, is_test_edit: 0,
        is_write_src: /write|create|save|append/i.test(tool) ? 1 : 0,
        is_outside_repo: 0,
        tainted: tt.classes.some(c => c !== 'user') ? 1 : 0, from_user: tt.classes.includes('user') ? 1 : 0, tainted_from: tt.classes.join(' '),
        tainted_by: tt.ids, taint_snip: tt.snippet.slice(0, 120),
        label: 'mcphunt', attack: t.task_id || t.trace_id || '', harm_step: -1, is_harm: isHarm,
        repo: t.risk_mechanism || '', resolved: 0,
      });
      // record this event's output for later taint
      taint.addOutput(id, cls, String(e.result_preview || ''));
      seq++;
    }
    if (harm < 0) harm = Math.max(0, seq - 1);
    for (const s of stepRows) s.harm_step = harm;
    if (!stepRows.some(x => x.is_harm) && stepRows[harm]) stepRows[harm].is_harm = 1;
    steps.push(...stepRows);
    sidx++;
  }
  return { steps, n: kept, nextSidx: sidx };
}

// ---------- preset policies -------------------------------------------------------------
const RULES = [
  { id: 'pkg', name: 'Package installs ask', action: 'ask', predicate: 'is_pkg_install = 1', frontier: 1,
    descr: 'Any pip/npm/apt/conda install prompts before it runs.' },
  { id: 'del', name: 'File deletes ask', action: 'ask', predicate: 'is_delete = 1', frontier: 1,
    descr: 'rm / unlink / rmtree prompts before deleting.' },
  { id: 'test', name: 'Edits to tests ask', action: 'ask', predicate: 'is_test_edit = 1', frontier: 1,
    descr: 'Writing to a test file prompts (guarding against an agent weakening its own tests).' },
  { id: 'egress', name: 'Network egress denies', action: 'deny', predicate: 'is_egress = 1', frontier: 1,
    descr: 'Any outbound network action (curl, wget, send, upload, push) is blocked.' },
  { id: 'outside', name: 'Writes outside the repo deny', action: 'deny', predicate: 'is_outside_repo = 1', frontier: 1,
    descr: 'A file write outside the working tree is blocked.' },
  { id: 'taint_any', name: 'Taint: command built from any tool output asks', action: 'ask',
    predicate: "kind = 'command' AND tainted = 1", frontier: 1,
    descr: "Nodwyn's provenance rule, as written: a command whose arguments reuse earlier tool output must ask." },
  { id: 'taint_untrusted', name: 'Taint from untrusted sources only asks', action: 'ask',
    predicate: "kind = 'command' AND (tainted_from LIKE '%web%' OR tainted_from LIKE '%mcp%')", frontier: 1,
    descr: 'The same rule, limited to output that came from the web or an MCP tool.' },
  { id: 'combo', name: 'Egress OR untrusted-taint denies', action: 'deny',
    predicate: "is_egress = 1 OR tainted_from LIKE '%web%' OR tainted_from LIKE '%mcp%'", frontier: 1,
    descr: 'A combined policy: block egress and anything built from untrusted output.' },
];

// ---------- run -------------------------------------------------------------------------
async function importParquet() {
  const buf = await readFile(PARQUET);
  // temp table: recreate clean each run
  try { await db.sql('DROP TABLE timemachine_import'); } catch {}
  await db.sql('CREATE TABLE timemachine_import (trajectory_id KEYWORD PRIMARY KEY, instance_id KEYWORD, repo KEYWORD, resolved INT, exit_status KEYWORD, trajectory TEXT, tools TEXT)');
  // raw fetch: the binary body must not be JSON-encoded.
  const res = await fetch(db.BASE + '/v1/import?format=parquet&table=timemachine_import&key=trajectory_id',
    { method: 'POST', headers: { 'content-type': 'application/vnd.apache.parquet', ...(process.env.CHLOEDB_TOKEN ? { authorization: `Bearer ${process.env.CHLOEDB_TOKEN}` } : {}) }, body: buf });
  const txt = await res.text();
  if (!res.ok) throw new Error(`import ${res.status}: ${txt.slice(0, 300)}`);
  return JSON.parse(txt);
}

async function main() {
  console.log('[prep] importing parquet into a temp table on', db.BASE);
  const imp = await importParquet();
  console.log('[prep] imported', imp.rows, 'trajectories');
  const raw = await db.query('SELECT trajectory_id, repo, resolved, trajectory FROM timemachine_import LIMIT 1000');
  console.log('[prep] normalizing', raw.rows.length, 'benign sessions');
  const benign = [];
  raw.rows.forEach((row, i) => { benign.push(...normalizeOpenHands(row, i)); });
  try { await db.sql('DROP TABLE timemachine_import'); } catch {}

  const ad = await normalizeAgentDojo(0);
  const mh = await normalizeMcpHunt(ad.nextSidx);
  const attacks = [...ad.steps, ...mh.steps];
  console.log(`[prep] benign steps ${benign.length} (${raw.rows.length} sessions); attacks ${attacks.length} (agentdojo ${ad.n}, mcphunt ${mh.n})`);

  // write artifacts
  await writeFile(path.join(DATA, 'benign.jsonl'), benign.map(s => JSON.stringify(s)).join('\n') + '\n');
  await writeFile(path.join(DATA, 'attacks.jsonl'), attacks.map(s => JSON.stringify(s)).join('\n') + '\n');
  await writeFile(path.join(DATA, 'rules.json'), JSON.stringify(RULES, null, 2));

  const benignSessions = raw.rows.length;
  const taintBenign = benign.filter(s => s.kind === 'command' && s.tainted).length;
  const cmds = benign.filter(s => s.kind === 'command').length;
  const meta = {
    built_at: new Date().toISOString(),
    datasets: [
      { name: 'OpenHands SWE-rebench trajectories (benign history)', file: 'nebius-SWE-rebench-openhands-trajectories/sample-300.parquet',
        sessions: benignSessions, steps: benign.length, licence: 'see dataset card (nebius / SWE-rebench, OpenHands)' },
      { name: 'AgentDojo recorded runs (labelled attacks: attacker goal reached)', file: 'agentdojo/repo/runs',
        sessions: ad.n, steps: ad.steps.length, licence: 'MIT (ethz-spylab/agentdojo)',
        flag_meaning: 'security=true means the injected task goal was achieved (verified by reading tool calls); those runs are the labelled set.' },
      { name: 'MCPHunt agent traces (labelled attacks: unsafe_success)', file: 'mcphunt-agent-traces/deepseek_v4_flash_baseline.json',
        sessions: mh.n, steps: mh.steps.length, licence: 'CC-BY-4.0' },
    ],
    totals: { benign_sessions: benignSessions, benign_steps: benign.length, benign_commands: cmds,
      attack_sessions: ad.n + mh.n, attack_steps: attacks.length,
      labelled_n: ad.n + mh.n, agentdojo_n: ad.n, mcphunt_n: mh.n },
    taint: { method: 'approximated from the record by shared substrings',
      minlen: MINLEN, benign_commands_tainted: taintBenign,
      note: 'tainted_by edges link a step to earlier outputs in the same session sharing a verbatim run of >= 20 characters with its arguments; source class is of the earlier output (web, mcp, file, test output, user). This is NOT Nodwyn value-level taint.' },
    wall: { lanes: benignSessions, cells: benign.length },
  };
  await writeFile(path.join(DATA, 'meta.json'), JSON.stringify(meta, null, 2));

  // sizes
  for (const f of ['benign.jsonl', 'attacks.jsonl']) {
    const st = await stat(path.join(DATA, f));
    console.log(`[prep] data/${f}  ${(st.size / 1e6).toFixed(2)} MB`);
  }
  console.log('[prep] done. taint fires on', taintBenign, 'of', cmds, 'benign commands',
    `(${(100 * taintBenign / cmds).toFixed(0)}%)`);
}

main().catch(e => { console.error('[prep] FAILED', e); process.exit(1); });

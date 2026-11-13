// The model behind the forge. It runs Claude through the Claude Code CLI
// (`claude -p`), so it uses whatever sign-in Claude Code has on this machine
// (claude.ai login or ANTHROPIC_API_KEY) and needs no key of its own. Tools,
// settings, plugins and MCP servers are switched off: the model only answers.

import { spawn } from 'node:child_process';
import { AsyncLocalStorage } from 'node:async_hooks';
import { readFile, writeFile, rename } from 'node:fs/promises';

export const MODEL = process.env.STRATA_APPS_MODEL || 'claude-opus-5-5';
const CLAUDE = process.env.STRATA_APPS_CLAUDE || 'claude';

// A cap on what the model may cost here, for a studio other people can
// reach: STRATA_APPS_SPEND_CAP_USD, in US dollars, counted from the cost
// Claude Code reports for each answer and kept in a file (useSpendFile) so a
// restart does not reset it. Once the total reaches the cap every call is
// refused until the cap is raised. Calls already running finish, so the total
// can pass the cap by what they cost; a call stopped before its answer is not
// counted.
//
// Apps' own model calls (app.ask, which a published app's visitors set off)
// count against a share of the cap of their own, `asksCap`
// (STRATA_APPS_ASK_CAP_USD, by default a quarter of the cap), and builds
// against the rest, so that traffic to apps never stops the studio's builds,
// nor builds the apps (audit N-G3).
const CAP = Number(process.env.STRATA_APPS_SPEND_CAP_USD) || 0;
const ASKS_CAP = process.env.STRATA_APPS_ASK_CAP_USD !== undefined && process.env.STRATA_APPS_ASK_CAP_USD !== '' ? Math.max(0, Number(process.env.STRATA_APPS_ASK_CAP_USD) || 0) : CAP / 4;
export const spend = { cap: CAP, asksCap: CAP ? Math.min(ASKS_CAP, CAP) : ASKS_CAP, usd: 0, asksUsd: 0, calls: 0, refused: 0, since: new Date().toISOString(), file: null };
let saving = Promise.resolve();
export async function useSpendFile(file) {
  spend.file = file;
  try {
    const j = JSON.parse(await readFile(file, 'utf8'));
    if (Number.isFinite(j.usd)) Object.assign(spend, { usd: j.usd, asksUsd: Number(j.asksUsd) || 0, calls: j.calls || 0, refused: j.refused || 0, since: j.since || spend.since });
  } catch { /* first start */ }
  return spend;
}
function saveSpend() {
  if (!spend.file) return;
  const { usd, asksUsd, calls, refused, since, cap, asksCap } = spend;
  saving = saving.then(() => writeFile(`${spend.file}.tmp`, JSON.stringify({ usd, asksUsd, calls, refused, since, cap, asksCap }))
    .then(() => rename(`${spend.file}.tmp`, spend.file))).catch(() => {});
}
// Why a call of `bucket` ('build': the studio's own; 'asks': an app's
// app.ask) may not run now, if it may not.
export function overCap(bucket = 'build') {
  const refuse = (text) => {
    spend.refused++;
    saveSpend();
    return Object.assign(new Error(text), { status: 402, refused: true });
  };
  if (bucket === 'asks') {
    if (spend.asksCap && spend.asksUsd >= spend.asksCap) return refuse(`this studio's apps have reached their share of its spending limit for the model ($${spend.asksUsd.toFixed(2)} of $${spend.asksCap}); the studio's builds go on; ask whoever runs it to raise STRATA_APPS_ASK_CAP_USD`);
    if (spend.cap && spend.usd >= spend.cap) return refuse(`this studio has reached its spending limit for the model ($${spend.usd.toFixed(2)} of $${spend.cap}); ask whoever shared it to raise the limit`);
    return null;
  }
  if (!spend.cap) return null;
  const builds = spend.usd - spend.asksUsd;
  const share = spend.cap - spend.asksCap;
  if (builds < share) return null;
  return refuse(`this studio has reached its spending limit for the model ($${builds.toFixed(2)} of $${share} for builds${spend.asksCap ? ', the rest kept for apps\' own calls' : ''}); ask whoever shared it to raise the limit`);
}

function counted(cost, bucket) {
  spend.usd += cost;
  if (bucket === 'asks') spend.asksUsd += cost;
  spend.calls++;
  saveSpend();
}

// How long one answer may take. A build is stopped when the model goes
// quiet (nothing new from it, not even a line of thinking, for IDLE seconds),
// runs past the hard cap, or its owner stops it (buildScope), so a big app
// that is still being written is not cut off, and one that says nothing does
// not hold its app for minutes.
export const IDLE_S = Number(process.env.STRATA_APPS_IDLE_S) || 90;
export const TIMEOUT_S = Number(process.env.STRATA_APPS_TIMEOUT_S) || 1200;

// The build a call belongs to, as the server runs it: buildScope.run({signal},
// fn). Every model call made inside it (the plan, each piece, each fix
// round) stops at once when the signal aborts, with an error marked stopped.
export const buildScope = new AsyncLocalStorage();
export const stoppedError = () => Object.assign(new Error('stopped by its owner'), { stopped: true });

// onWrite(progress, text) is called as the answer streams in, with
// {chars, thinking}: characters of answer and of thinking so far, and the
// answer so far. images: [{data (base64), type ('image/png')}] go before the
// prompt, in one message.
// tools: built-in tools the model may use (WebSearch, WebFetch for an app's
// web ask); none by default. maxBudgetUsd stops a call that costs more.
// bucket: 'asks' for an app's own call (app.ask), counted apart (overCap).
export function ask({ system, prompt, images = [], model = MODEL, effort = process.env.STRATA_APPS_EFFORT, idleMs = IDLE_S * 1000, timeoutMs = TIMEOUT_S * 1000, onWrite, bin = CLAUDE, tools = [], maxBudgetUsd, bucket = 'build' }) {
  const over = overCap(bucket);
  if (over) return Promise.reject(over);
  const signal = buildScope.getStore()?.signal;
  if (signal?.aborted) return Promise.reject(stoppedError());
  return new Promise((resolve, reject) => {
    const args = ['-p', '--output-format', 'stream-json', '--verbose', '--include-partial-messages', '--model', model, '--system-prompt', system,
      '--tools', tools.join(','), '--setting-sources', '', '--strict-mcp-config', '--disable-slash-commands', '--no-session-persistence'];
    if (tools.length) args.push('--allowedTools', tools.join(','));
    if (maxBudgetUsd) args.push('--max-budget-usd', String(maxBudgetUsd));
    if (effort) args.push('--effort', effort);
    if (images.length) args.push('--input-format', 'stream-json');
    const env = { ...process.env, CHLOECTX_DISABLE: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' };
    env.ENGRAM_DISABLE = '1'; // read by ChloeCtx's hooks from before the rename
    const child = spawn(bin, args, { env, stdio: ['pipe', 'pipe', 'pipe'], cwd: process.env.TMPDIR || '/tmp' });
    let buf = '';
    let err = '';
    let result = null;
    let text = '';
    const seen = { chars: 0, thinking: 0 };
    let settled = false;
    const onStop = () => fail(stoppedError());
    const fail = (e) => { if (settled) return; settled = true; clearTimeout(cap); clearTimeout(idle); signal?.removeEventListener('abort', onStop); child.kill('SIGKILL'); reject(e); };
    signal?.addEventListener('abort', onStop, { once: true });
    const cap = setTimeout(() => fail(new Error(`the model took longer than ${Math.round(timeoutMs / 1000)} s`)), timeoutMs);
    let idle;
    const quiet = () => {
      clearTimeout(idle);
      idle = setTimeout(() => fail(new Error(`the model wrote nothing for ${Math.round(idleMs / 1000)} s (${seen.chars} characters written)`)), idleMs);
    };
    quiet();
    const line = (l) => {
      if (!l.trim()) return;
      let j;
      try { j = JSON.parse(l); } catch { return; }
      quiet();
      if (j.type === 'result') { result = j; return; }
      if (j.type !== 'stream_event') return;
      const d = j.event?.type === 'content_block_delta' ? j.event.delta : null;
      if (!d) return;
      if (d.type === 'text_delta') { text += d.text; seen.chars += d.text.length; } else if (d.type === 'thinking_delta') seen.thinking += (d.thinking || '').length;
      else return;
      onWrite?.(seen, text);
    };
    child.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) { line(buf.slice(0, i)); buf = buf.slice(i + 1); }
    });
    child.stderr.on('data', (d) => { err += d; });
    child.on('error', (e) => fail(new Error(`could not run ${bin}: ${e.message}. Install Claude Code and sign in, or set STRATA_APPS_CLAUDE`)));
    child.on('close', (code) => {
      line(buf);
      if (settled) return;
      settled = true;
      clearTimeout(cap);
      clearTimeout(idle);
      signal?.removeEventListener('abort', onStop);
      const data = result;
      if (!data) return reject(new Error(`claude exited ${code}: ${(err || buf || text).slice(0, 600)}`));
      if (data.is_error || data.subtype !== 'success' && data.subtype) {
        if (data.total_cost_usd) counted(data.total_cost_usd, bucket);
        return reject(new Error(`claude: ${data.result || data.terminal_reason || data.subtype}`));
      }
      const out = { text: data.result || text, usage: data.usage || {}, cost: data.total_cost_usd || 0, ms: data.duration_ms || 0, model };
      counted(out.cost, bucket);
      // STRATA_APPS_DUMP=dir keeps every answer there, to study what the model writes.
      if (process.env.STRATA_APPS_DUMP) {
        import('node:fs').then((fs) => fs.promises.mkdir(process.env.STRATA_APPS_DUMP, { recursive: true })
          .then(() => fs.promises.writeFile(`${process.env.STRATA_APPS_DUMP}/${Date.now()}-${model}.json`, JSON.stringify({ model, effort: effort || null, ms: out.ms, cost: out.cost, usage: out.usage, thinking: seen.thinking, prompt: prompt.length, text: out.text }))))
          .catch(() => {});
      }
      resolve(out);
    });
    child.stdin.end(!images.length ? prompt : `${JSON.stringify({ type: 'user', message: { role: 'user', content: [
      ...images.map((i) => ({ type: 'image', source: { type: 'base64', media_type: i.type || 'image/png', data: i.data } })),
      { type: 'text', text: prompt }] } })}\n`);
  });
}

// The model answers with a plan, as JSON inside <plan>...</plan>, and the
// code of each capability in its own <code slug="...">...</code> block, so
// code never has to be escaped inside JSON.
export function parseAnswer(text) {
  let planText = null;
  const p = text.match(/<plan>\s*([\s\S]*?)\s*<\/plan>/);
  if (p) planText = p[1].replace(/^```(?:json)?\s*/, '').replace(/```\s*$/, '');
  else {
    const fence = text.match(/```(?:json)?\s*\n([\s\S]*?)\n```/);
    if (fence) planText = fence[1];
  }
  if (!planText) throw new Error(`the answer held no <plan>: ${text.slice(0, 300)}`);
  let plan;
  try { plan = JSON.parse(planText); } catch (e) { throw new Error(`the plan is not valid JSON (${e.message}): ${planText.slice(0, 300)}`); }
  const code = {};
  for (const m of text.matchAll(/<code\s+slug="([^"]+)"\s*>\n?([\s\S]*?)\n?<\/code>/g)) code[m[1]] = m[2];
  return { plan, code };
}

export function extractJson(text) {
  const fence = text.match(/```(?:json)?\s*\n([\s\S]*?)\n```/);
  const candidates = [];
  if (fence) candidates.push(fence[1]);
  const a = text.indexOf('{');
  const b = text.lastIndexOf('}');
  if (a >= 0 && b > a) candidates.push(text.slice(a, b + 1));
  for (const c of candidates) {
    try { return JSON.parse(c); } catch { /* next */ }
  }
  throw new Error(`the model's answer held no JSON object: ${text.slice(0, 300)}`);
}

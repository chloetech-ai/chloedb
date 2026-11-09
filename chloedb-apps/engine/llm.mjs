// The model behind the forge. It runs Claude through the Claude Code CLI
// (`claude -p`), so it uses whatever sign-in Claude Code has on this machine
// (claude.ai login or ANTHROPIC_API_KEY) and needs no key of its own. Tools,
// settings, plugins and MCP servers are switched off: the model only answers.

import { spawn } from 'node:child_process';
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
export const spend = { cap: Number(process.env.STRATA_APPS_SPEND_CAP_USD) || 0, usd: 0, calls: 0, refused: 0, since: new Date().toISOString(), file: null };
let saving = Promise.resolve();
export async function useSpendFile(file) {
  spend.file = file;
  try {
    const j = JSON.parse(await readFile(file, 'utf8'));
    if (Number.isFinite(j.usd)) Object.assign(spend, { usd: j.usd, calls: j.calls || 0, refused: j.refused || 0, since: j.since || spend.since });
  } catch { /* first start */ }
  return spend;
}
function saveSpend() {
  if (!spend.file) return;
  const { usd, calls, refused, since, cap } = spend;
  saving = saving.then(() => writeFile(`${spend.file}.tmp`, JSON.stringify({ usd, calls, refused, since, cap }))
    .then(() => rename(`${spend.file}.tmp`, spend.file))).catch(() => {});
}
function overCap() {
  if (!spend.cap || spend.usd < spend.cap) return null;
  spend.refused++;
  saveSpend();
  return Object.assign(new Error(`this studio has reached its spending limit for the model ($${spend.usd.toFixed(2)} of $${spend.cap}); ask whoever shared it to raise the limit`), { status: 402, refused: true });
}

// How long one answer may take. A build is stopped only when the model goes
// quiet (no new output for IDLE seconds) or runs past the hard cap, so a big
// app that is still being written is not cut off.
export const IDLE_S = Number(process.env.STRATA_APPS_IDLE_S) || 180;
export const TIMEOUT_S = Number(process.env.STRATA_APPS_TIMEOUT_S) || 1200;

// onWrite(progress, text) is called as the answer streams in, with
// {chars, thinking}: characters of answer and of thinking so far, and the
// answer so far. images: [{data (base64), type ('image/png')}] go before the
// prompt, in one message.
// tools: built-in tools the model may use (WebSearch, WebFetch for an app's
// web ask); none by default. maxBudgetUsd stops a call that costs more.
export function ask({ system, prompt, images = [], model = MODEL, effort = process.env.STRATA_APPS_EFFORT, idleMs = IDLE_S * 1000, timeoutMs = TIMEOUT_S * 1000, onWrite, bin = CLAUDE, tools = [], maxBudgetUsd }) {
  const over = overCap();
  if (over) return Promise.reject(over);
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
    const fail = (e) => { if (settled) return; settled = true; clearTimeout(cap); clearTimeout(idle); child.kill('SIGKILL'); reject(e); };
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
      const data = result;
      if (!data) return reject(new Error(`claude exited ${code}: ${(err || buf || text).slice(0, 600)}`));
      if (data.is_error || data.subtype !== 'success' && data.subtype) {
        if (data.total_cost_usd) { spend.usd += data.total_cost_usd; spend.calls++; saveSpend(); }
        return reject(new Error(`claude: ${data.result || data.terminal_reason || data.subtype}`));
      }
      const out = { text: data.result || text, usage: data.usage || {}, cost: data.total_cost_usd || 0, ms: data.duration_ms || 0, model };
      spend.usd += out.cost;
      spend.calls++;
      saveSpend();
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

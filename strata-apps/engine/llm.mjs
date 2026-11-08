// The model behind the forge. It runs Claude through the Claude Code CLI
// (`claude -p`), so it uses whatever sign-in Claude Code has on this machine
// (claude.ai login or ANTHROPIC_API_KEY) and needs no key of its own. Tools,
// settings, plugins and MCP servers are switched off: the model only answers.

import { spawn } from 'node:child_process';

export const MODEL = process.env.STRATA_APPS_MODEL || 'claude-opus-5-5';
const CLAUDE = process.env.STRATA_APPS_CLAUDE || 'claude';

// How long one answer may take. A build is stopped only when the model goes
// quiet (no new output for IDLE seconds) or runs past the hard cap, so a big
// app that is still being written is not cut off.
export const IDLE_S = Number(process.env.STRATA_APPS_IDLE_S) || 180;
export const TIMEOUT_S = Number(process.env.STRATA_APPS_TIMEOUT_S) || 1200;

// onWrite(progress) is called as the answer streams in, with
// {chars, thinking}: characters of answer and of thinking so far.
export function ask({ system, prompt, model = MODEL, effort = process.env.STRATA_APPS_EFFORT, idleMs = IDLE_S * 1000, timeoutMs = TIMEOUT_S * 1000, onWrite }) {
  return new Promise((resolve, reject) => {
    const args = ['-p', '--output-format', 'stream-json', '--verbose', '--include-partial-messages', '--model', model, '--system-prompt', system,
      '--tools', '', '--setting-sources', '', '--strict-mcp-config', '--disable-slash-commands', '--no-session-persistence'];
    if (effort) args.push('--effort', effort);
    const env = { ...process.env, ENGRAM_DISABLE: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' };
    const child = spawn(CLAUDE, args, { env, stdio: ['pipe', 'pipe', 'pipe'], cwd: process.env.TMPDIR || '/tmp' });
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
      onWrite?.(seen);
    };
    child.stdout.on('data', (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) { line(buf.slice(0, i)); buf = buf.slice(i + 1); }
    });
    child.stderr.on('data', (d) => { err += d; });
    child.on('error', (e) => fail(new Error(`could not run ${CLAUDE}: ${e.message}. Install Claude Code and sign in, or set STRATA_APPS_CLAUDE`)));
    child.on('close', (code) => {
      line(buf);
      if (settled) return;
      settled = true;
      clearTimeout(cap);
      clearTimeout(idle);
      const data = result;
      if (!data) return reject(new Error(`claude exited ${code}: ${(err || buf || text).slice(0, 600)}`));
      if (data.is_error || data.subtype !== 'success' && data.subtype) {
        return reject(new Error(`claude: ${data.result || data.terminal_reason || data.subtype}`));
      }
      resolve({ text: data.result || text, usage: data.usage || {}, cost: data.total_cost_usd || 0, ms: data.duration_ms || 0, model });
    });
    child.stdin.end(prompt);
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

// The model behind the forge. It runs Claude through the Claude Code CLI
// (`claude -p`), so it uses whatever sign-in Claude Code has on this machine
// (claude.ai login or ANTHROPIC_API_KEY) and needs no key of its own. Tools,
// settings, plugins and MCP servers are switched off: the model only answers.

import { spawn } from 'node:child_process';

export const MODEL = process.env.STRATA_APPS_MODEL || 'claude-opus-5-5';
const CLAUDE = process.env.STRATA_APPS_CLAUDE || 'claude';

export function ask({ system, prompt, model = MODEL, effort, timeoutMs = 300000 }) {
  return new Promise((resolve, reject) => {
    const args = ['-p', '--output-format', 'json', '--model', model, '--system-prompt', system,
      '--tools', '', '--setting-sources', '', '--strict-mcp-config', '--disable-slash-commands', '--no-session-persistence'];
    if (effort) args.push('--effort', effort);
    const env = { ...process.env, ENGRAM_DISABLE: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1' };
    const child = spawn(CLAUDE, args, { env, stdio: ['pipe', 'pipe', 'pipe'], cwd: process.env.TMPDIR || '/tmp' });
    let out = '';
    let err = '';
    const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error(`the model took longer than ${Math.round(timeoutMs / 1000)} s`)); }, timeoutMs);
    child.stdout.on('data', (d) => { out += d; });
    child.stderr.on('data', (d) => { err += d; });
    child.on('error', (e) => { clearTimeout(timer); reject(new Error(`could not run ${CLAUDE}: ${e.message}. Install Claude Code and sign in, or set STRATA_APPS_CLAUDE`)); });
    child.on('close', (code) => {
      clearTimeout(timer);
      let data;
      try { data = JSON.parse(out); } catch {
        return reject(new Error(`claude exited ${code}: ${(err || out).slice(0, 600)}`));
      }
      if (data.is_error || data.subtype !== 'success' && data.subtype) {
        return reject(new Error(`claude: ${data.result || data.terminal_reason || data.subtype}`));
      }
      resolve({ text: data.result || '', usage: data.usage || {}, cost: data.total_cost_usd || 0, ms: data.duration_ms || 0, model });
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

// Arm C of the matrix: Nodwyn's real session, driven through harness/leakgate (a Go
// command built inside a copy of the Nodwyn repository; see harness/leakgate/main.go).
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { HOSTS, httpRows } from './world.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
export const LEAKGATE = process.env.LEAKLAB_LEAKGATE || path.join(here, '..', 'data', 'leakgate');
export const haveLeakgate = () => existsSync(LEAKGATE);

function run(input, timeoutMs = 300000) {
  const r = spawnSync(LEAKGATE, [], { input: JSON.stringify(input), encoding: 'utf8', maxBuffer: 1 << 30, timeout: timeoutMs, env: { PATH: process.env.PATH, HOME: process.env.HOME || '/tmp' } });
  if (r.status !== 0) throw new Error(`leakgate exited ${r.status}: ${(r.stderr || '').slice(0, 800)}`);
  return r.stdout.trim().split('\n').filter(Boolean).map(l => JSON.parse(l));
}

// Scenarios through a real session each. policy 'n' or 'y' answers every confirm.
export function runScenarios(canary, policy, scenarios) {
  return run({
    canary: canary.value, canary_name: canary.name, policy,
    scenarios: scenarios.map(s => ({ id: s.id, scm: s.scm.replaceAll('{{NAME}}', canary.name), mcp: Boolean(s.mcp) })),
  });
}

// Direct calls of binding.Lookup and redact.Redact (no session).
export function runOps(ops) { return run({ ops }); }

// Every byte a session let out, as rows of the same shape the simulated arms produce.
export function rowsOf(res) {
  const rows = [];
  for (const h of res.http || []) {
    const sink = h.host === HOSTS.mcp ? 'mcp' : 'http';
    const hdrs = Object.fromEntries(Object.entries(h.headers || {}).filter(([k]) => !['Accept-Encoding', 'Content-Length', 'User-Agent', 'Content-Type'].includes(k)));
    const rs = httpRows({ host: h.host, method: h.method, path: h.path, query: h.query, headers: hdrs, body: h.body, sink });
    if (sink === 'mcp') for (const r of rs) r.authorised = r.position === 'header:Authorization' ? 1 : 0;
    rows.push(...rs);
  }
  for (const [i, raw] of (res.model_requests || []).entries()) {
    rows.push({ sink: 'model_request', host: HOSTS.provider, method: 'POST', position: `request ${i + 1}`, raw });
  }
  for (const line of (res.log || '').split('\n')) {
    if (!line) continue;
    let kind = 'event';
    try { kind = JSON.parse(line).kind; } catch { /* keep the line as it is */ }
    rows.push({ sink: kind === 'llm.resp' ? 'answer' : 'record', host: kind === 'llm.resp' ? 'user' : HOSTS.local, method: '', position: kind, raw: line });
  }
  if (res.output) rows.push({ sink: 'answer', host: 'user', method: '', position: 'scheme output', raw: res.output });
  for (const f of res.files || []) {
    const sink = f.path.includes('COMMIT_EDITMSG') ? 'commit' : f.path.startsWith('memory/') || f.path.includes('notes') ? 'memory' : 'file';
    rows.push({ sink, host: HOSTS.local, method: '', position: f.path, raw: f.content });
  }
  if (res.tools) {
    // What the stand-ins for curl, wget and nc were given: the bytes a real one would send.
    for (const block of res.tools.split(/^end$/m)) {
      if (!block.trim()) continue;
      const url = (block.match(/^arg=(https?:\/\/\S+)/m) || [])[1];
      let host = 'network tool';
      try { if (url) host = new URL(url).hostname; } catch { /* no address */ }
      const [args, env = ''] = block.split(/^env:$/m);
      rows.push({ sink: 'subprocess', host, method: 'POST', position: 'argv and files', raw: args.trim() });
      rows.push({ sink: 'child_env', host: HOSTS.local, method: '', position: 'child environment', raw: env.trim(), note: 'the environment the child process saw (not sent unless the command sends it)' });
    }
  }
  for (const r of rows) r.authorised = r.authorised ? 1 : 0;
  return rows;
}

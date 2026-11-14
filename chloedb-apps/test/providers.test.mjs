// The model's providers (engine/llm.mjs): claude, codex and agy. Their
// arguments are built by pure functions; codex runs against a fake `codex`
// that prints the events `codex exec --json` prints. No real model calls.

import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, chmodSync, mkdtempSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { ask, claudeArgs, codexArgs, codexEvent, checkProvider, withProvider, currentProvider, modelFor, PROVIDER, AGY_UNSUPPORTED } from '../engine/llm.mjs';

test('claude: the same arguments as before', () => {
  assert.deepEqual(claudeArgs({ system: 'S', model: 'claude-x' }), ['-p', '--output-format', 'stream-json', '--verbose', '--include-partial-messages', '--model', 'claude-x', '--system-prompt', 'S',
    '--tools', '', '--setting-sources', '', '--strict-mcp-config', '--disable-slash-commands', '--no-session-persistence']);
  const web = claudeArgs({ system: 'S', model: 'm', tools: ['WebSearch', 'WebFetch'], maxBudgetUsd: 0.5, effort: 'low', images: [{}] });
  assert.deepEqual(web.slice(-8), ['--allowedTools', 'WebSearch,WebFetch', '--max-budget-usd', '0.5', '--effort', 'low', '--input-format', 'stream-json']);
  assert.equal(web[web.indexOf('--tools') + 1], 'WebSearch,WebFetch');
});

test('codex: exec, read only, no tools, no user config, answer only', () => {
  const a = codexArgs({ system: 'Be "brief"\nnow', model: 'gpt-5.5', effort: 'high', images: ['/t/a.png'], outFile: '/t/out.txt', cwd: '/t/w' });
  assert.equal(a[0], 'exec');
  assert.equal(a.at(-1), '-', 'the prompt is read from stdin');
  const val = (f) => a[a.indexOf(f) + 1];
  assert.ok(a.includes('--json') && a.includes('--ephemeral') && a.includes('--ignore-user-config') && a.includes('--ignore-rules') && a.includes('--skip-git-repo-check'));
  assert.equal(val('--sandbox'), 'read-only');
  assert.equal(val('--model'), 'gpt-5.5');
  assert.equal(val('--cd'), '/t/w');
  assert.equal(val('--output-last-message'), '/t/out.txt');
  assert.equal(val('--image'), '/t/a.png');
  const disabled = a.flatMap((x, i) => (x === '--disable' ? [a[i + 1]] : []));
  for (const f of ['shell_tool', 'apps', 'browser_use', 'computer_use', 'image_generation']) assert.ok(disabled.includes(f), f);
  const conf = a.flatMap((x, i) => (x === '-c' ? [a[i + 1]] : []));
  assert.ok(conf.includes('web_search="disabled"'));
  assert.ok(conf.includes('approval_policy="never"'));
  assert.ok(conf.includes('model_reasoning_effort="high"'));
  assert.ok(conf.includes('developer_instructions="Be \\"brief\\"\\nnow"'), 'the system prompt as a TOML string');
  // No model: the CLI's own; Claude's efforts it does not know are left out.
  const b = codexArgs({ system: 's', effort: 'max' });
  assert.ok(!b.includes('--model'));
  assert.ok(!b.some((x) => x.startsWith('model_reasoning_effort')));
});

test('codex: its JSONL events', () => {
  assert.deepEqual(codexEvent('{"type":"item.completed","item":{"id":"i","type":"agent_message","text":"hi"}}'), { text: 'hi', done: true });
  assert.deepEqual(codexEvent('{"type":"item.completed","item":{"type":"reasoning","text":"hmm"}}'), { thinking: 'hmm' });
  assert.deepEqual(codexEvent('{"type":"turn.completed","usage":{"input_tokens":5,"output_tokens":2}}'), { usage: { input_tokens: 5, output_tokens: 2 } });
  assert.deepEqual(codexEvent('{"type":"turn.failed","error":{"message":"401 Unauthorized"}}'), { error: '401 Unauthorized' });
  assert.equal(codexEvent('not json'), null);
});

test('providers: chosen by name, per build, with models of their own', async () => {
  assert.equal(checkProvider(undefined), null);
  assert.equal(checkProvider('Codex'), 'codex');
  assert.throws(() => checkProvider('gpt'), (e) => e.status === 400 && /claude, codex, agy/.test(e.message));
  assert.equal(PROVIDER, 'claude', 'claude unless STRATA_APPS_PROVIDER says otherwise');
  assert.equal(currentProvider(), 'claude');
  const inside = await withProvider('codex', async () => { await new Promise((r) => setTimeout(r, 5)); return currentProvider(); });
  assert.equal(inside, 'codex', 'kept across awaits');
  assert.equal(currentProvider(), 'claude', 'and only inside');
  assert.equal(await withProvider(null, async () => currentProvider()), 'claude');
  assert.equal(modelFor('claude', 'claude-haiku-5-5'), 'claude-haiku-5-5');
  assert.equal(modelFor('codex', 'claude-haiku-5-5'), null, 'Claude\'s names are not sent to codex');
  assert.equal(modelFor('codex', 'gpt-5.5'), 'gpt-5.5');
});

test('agy: refused with a clear error', async () => {
  await assert.rejects(ask({ system: 's', prompt: 'p', provider: 'agy' }), (e) => e.message === AGY_UNSUPPORTED && e.status === 400);
  await assert.rejects(withProvider('agy', () => ask({ system: 's', prompt: 'p' })), /not supported/);
});

// A fake `codex`: writes its arguments and stdin, prints events like codex.
function fakeCodex({ fail = false } = {}) {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'sa-fake-codex-'));
  const bin = path.join(dir, 'codex');
  writeFileSync(bin, `#!${process.execPath}
const fs = require('fs');
const args = process.argv.slice(2);
let input = '';
process.stdin.on('data', (d) => { input += d; });
process.stdin.on('end', () => {
  fs.writeFileSync(${JSON.stringify(path.join(dir, 'seen.json'))}, JSON.stringify({ args, input, cwd: process.cwd() }));
  const out = (o) => process.stdout.write(JSON.stringify(o) + '\\n');
  out({ type: 'thread.started', thread_id: 't' });
  out({ type: 'turn.started' });
  if (${fail}) { out({ type: 'turn.failed', error: { message: 'unexpected status 401 Unauthorized' } }); process.exit(1); }
  out({ type: 'item.completed', item: { id: 'r', type: 'reasoning', text: 'thinking' } });
  out({ type: 'item.completed', item: { id: 'm', type: 'agent_message', text: '<plan>{}</plan>' } });
  const o = args[args.indexOf('--output-last-message') + 1];
  fs.writeFileSync(o, '<plan>{}</plan>');
  out({ type: 'turn.completed', usage: { input_tokens: 10, output_tokens: 4 } });
});
`);
  chmodSync(bin, 0o755);
  return { bin, seen: () => JSON.parse(readFileSync(path.join(dir, 'seen.json'), 'utf8')) };
}

test('codex: an answer through a fake codex, costing 0', async () => {
  const fake = fakeCodex();
  const writes = [];
  const out = await ask({ provider: 'codex', bin: fake.bin, system: 'SYS', prompt: 'build a counter', model: 'claude-sonnet-5-5', onWrite: (w) => writes.push({ ...w }) });
  assert.equal(out.text, '<plan>{}</plan>');
  assert.equal(out.cost, 0);
  assert.equal(out.costReported, false);
  assert.equal(out.provider, 'codex');
  assert.deepEqual(out.usage, { input_tokens: 10, output_tokens: 4 });
  const seen = fake.seen();
  assert.equal(seen.input, 'build a counter');
  assert.ok(!seen.args.includes('claude-sonnet-5-5'));
  assert.ok(seen.args.includes('developer_instructions="SYS"'));
  assert.ok(writes.length >= 2 && writes.at(-1).chars === out.text.length);
  // Through a build's provider.
  const again = await withProvider('codex', () => ask({ bin: fake.bin, system: 's', prompt: 'p' }));
  assert.equal(again.provider, 'codex');
  // Tools (an app's web ask) need claude.
  await assert.rejects(ask({ provider: 'codex', bin: fake.bin, system: 's', prompt: 'p', tools: ['WebSearch'] }), /needs the claude provider/);
});

test('codex: a failed turn is the error', async () => {
  const fake = fakeCodex({ fail: true });
  await assert.rejects(ask({ provider: 'codex', bin: fake.bin, system: 's', prompt: 'p' }), /codex: unexpected status 401/);
});

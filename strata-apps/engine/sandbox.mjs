// Server functions run here: QuickJS compiled to WebAssembly, one fresh
// interpreter per call, with a memory limit, a deadline, no network, no files
// and no Node APIs. The only way out is `app`, whose calls the host answers
// (and checks) through one asyncified function, so the guest's code reads
// like ordinary synchronous JavaScript.

import { newAsyncContext, newQuickJSWASMModule, shouldInterruptAfterDeadline } from 'quickjs-emscripten';

const PRELUDE = `
const __call = (m, a) => { const r = JSON.parse(__host(m, JSON.stringify(a === undefined ? [] : a))); if (r.err !== undefined) throw new Error(r.err); return r.ok; };
const __fmt = (a) => a.map(v => typeof v === 'string' ? v : JSON.stringify(v)).join(' ');
const console = { log: (...a) => __log(__fmt(a)), info: (...a) => __log(__fmt(a)), warn: (...a) => __log(__fmt(a)), error: (...a) => __log(__fmt(a)) };
const app = Object.freeze({
  sql: (sql, params) => __call('sql', [sql, params || {}]),
  insert: (table, doc) => __call('insert', [table, doc || {}]),
  update: (table, id, fields) => __call('update', [table, id, fields || {}]),
  remove: (table, id) => __call('remove', [table, id]),
  call: (name, input) => __call('call', [name, input === undefined ? null : input]),
  now: () => new Date().toISOString(),
  log: (...a) => __log(__fmt(a)),
});
`;

function errorText(dumped) {
  if (dumped && typeof dumped === 'object') {
    const msg = dumped.message || dumped.name || JSON.stringify(dumped);
    return dumped.stack ? `${msg}\n${String(dumped.stack).split('\n').slice(0, 3).join('\n')}` : msg;
  }
  return String(dumped);
}

// Checks that `code` parses in QuickJS and defines run(), without running it.
let syncModule = null;
export async function checkFunction(code) {
  const problems = [];
  if (/\basync\s+function\s+run\b|\brun\s*=\s*async\b/.test(code)) problems.push('run must be a plain synchronous function: app calls return their results directly, so do not use async or await');
  if (/\bawait\b/.test(code)) problems.push('do not use await: app.sql, app.insert and the other app calls return their results directly');
  if (code.length > 64 * 1024) problems.push('the code is longer than 64 KiB');
  syncModule ||= await newQuickJSWASMModule();
  const vm = syncModule.newContext();
  try {
    vm.runtime.setInterruptHandler(shouldInterruptAfterDeadline(Date.now() + 1000));
    // Compile only: wrap in a function that is never called.
    const res = vm.evalCode(`(function () {\n${code}\n;return typeof run;\n})`, 'check.js');
    if (res.error) { problems.push('syntax: ' + errorText(vm.dump(res.error))); res.error.dispose(); }
    else res.value.dispose();
  } finally {
    vm.dispose();
  }
  if (!/\bfunction\s+run\s*\(|\brun\s*=\s*(function|\()/.test(code)) problems.push('the code must define function run(input, app)');
  return problems;
}

// Runs `code`'s run(input, app). `host(method, args)` answers app calls.
export async function runFunction(code, input, host, { timeoutMs = 10000, memoryBytes = 64 * 1024 * 1024 } = {}) {
  const vm = await newAsyncContext();
  const logs = [];
  const started = Date.now();
  try {
    vm.runtime.setMemoryLimit(memoryBytes);
    vm.runtime.setMaxStackSize(1024 * 1024);
    vm.runtime.setInterruptHandler(shouldInterruptAfterDeadline(started + timeoutMs));
    const hostFn = vm.newAsyncifiedFunction('__host', async (mH, aH) => {
      const method = vm.getString(mH);
      const args = JSON.parse(vm.getString(aH));
      try {
        if (Date.now() - started > timeoutMs) throw new Error('the function ran out of time');
        const out = await host(method, args);
        return vm.newString(JSON.stringify({ ok: out === undefined ? null : out }));
      } catch (e) {
        return vm.newString(JSON.stringify({ err: String(e && e.message || e) }));
      }
    });
    vm.setProp(vm.global, '__host', hostFn);
    hostFn.dispose();
    const logFn = vm.newFunction('__log', (h) => { if (logs.length < 200) logs.push(vm.getString(h)); });
    vm.setProp(vm.global, '__log', logFn);
    logFn.dispose();
    const arg = JSON.stringify(input === undefined ? null : input);
    const program = `${PRELUDE}\n${code}\n;globalThis.__last = run(${arg}, app);\nJSON.stringify({ result: globalThis.__last === undefined ? null : globalThis.__last });`;
    const res = await vm.evalCodeAsync(program, 'capability.js');
    if (res.error) {
      const text = errorText(vm.dump(res.error));
      res.error.dispose();
      const err = new Error(text);
      err.logs = logs;
      throw err;
    }
    const out = JSON.parse(vm.getString(res.value));
    res.value.dispose();
    return { result: out.result, logs, ms: Date.now() - started };
  } finally {
    vm.dispose();
  }
}

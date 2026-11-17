// The normaliser: seven on-disk trajectory formats of six agent harnesses -> one step shape.
//
// Each adapter below takes one session in its native format and returns
//   { meta: { native_id, source, harness, format, model, repo, instance, resolved, exit_status,
//             task, patch, feedback, started_at, dropped },
//     steps: [ { role, kind, tool, cmd, path, text, aux, exit_code, call } ] }
// where `call` (on an output) is the index, in `steps`, of the call it answers.
// finalise() then gives every step its id, its flags, its edges and its place in the session's
// hash chain. Nothing here reads a file or a database: prep.mjs does that, and Policy Time
// Machine (or anything else) can import these functions to load its own copy.
//
// The step shape
//   role  system | user | assistant | tool
//   kind  thought   what the agent said it was thinking (its prose before a call, a think tool)
//         command   a shell command, or a tool call that is not a file edit or a file view
//         edit      a call that writes a file: text is the content written, aux what it replaced
//         view      a call that reads a file
//         output    what a call returned
//         message   something said to or by the user (and the system prompt)
//         finish    the agent's own end of the session
//   links (EDGES, labelled)
//         output   a call -> what it returned
//         thought  any step -> the agent's latest thought before it
//         saw      a thought -> the latest output before it (what it was reacting to)
//         view     an edit -> the latest views (up to 3) of the same file before it
//         failed   an edit -> the latest failing output before it
import { createHash } from 'node:crypto';

export const TEXT_CAP = 24_000;      // characters of a step's text kept (head 18,000 + tail 6,000)
export const EDIT_CAP = 200_000;     // an edit's written content is kept longer: blame reads it
export const CHAIN_VERSION = 'blackbox/v1';

const sha = s => createHash('sha256').update(s).digest('hex');
const str = v => (v == null ? '' : typeof v === 'string' ? v : JSON.stringify(v));
// Text ChloeDB stores and returns byte for byte: no NUL, no lone surrogate.
export const clean = s => str(s).replace(/\u0000/g, '').toWellFormed();
const parse = (s, fallback = {}) => { if (s == null || s === '') return fallback; if (typeof s !== 'string') return s; try { return JSON.parse(s); } catch { return fallback; } };
const short = (v, n = 400) => { const s = typeof v === 'string' ? v : JSON.stringify(v ?? ''); return s.length > n ? s.slice(0, n) + '…' : s; };
// Anthropic-style content: a string, or blocks of which the text ones are joined.
const blocksText = c => (typeof c === 'string' ? c : Array.isArray(c) ? c.map(b => (typeof b === 'string' ? b : b?.type === 'text' ? b.text : b?.type === 'tool_reference' ? `[tool ${b.tool_name}]` : b?.text ?? '')).filter(Boolean).join('\n') : str(c));

const step = (role, kind, o = {}) => ({ role, kind, tool: '', cmd: '', path: '', text: '', aux: '', exit_code: null, call: null, ...o });

// ---- a tool call, whatever the harness names it --------------------------------------------
// OpenHands and SWE-agent (tool-call format) share str_replace_editor; Claude Code, Cursor,
// opencode and Swival name their tools differently. All of them come through here.
const SHELL = new Set(['execute_bash', 'bash', 'Bash', 'PowerShell', 'run_command', 'Shell', 'shell', 'run', 'terminal', 'run_terminal_cmd']);
const READ = new Set(['Read', 'read_file', 'read', 'view', 'open', 'ReadFile']);
const WRITE = new Set(['Write', 'write', 'write_file', 'create']);
const EDIT = new Set(['Edit', 'edit', 'edit_file', 'StrReplace', 'str_replace', 'MultiEdit', 'search_replace']);
const THINK = new Set(['think', 'TodoWrite', 'task_tracker', 'todowrite']);

export function toolCall(name, a = {}) {
  a = a && typeof a === 'object' ? a : {};
  const path = str(a.path ?? a.file_path ?? a.filePath ?? a.target_file ?? '');
  if (name === 'str_replace_editor') {
    const c = str(a.command);
    if (c === 'view') return step('assistant', 'view', { tool: name, path, cmd: `view ${path}${a.view_range ? ' ' + JSON.stringify(a.view_range) : ''}`, text: `view ${path}` });
    if (c === 'create') return step('assistant', 'edit', { tool: name, path, cmd: `create ${path}`, text: str(a.file_text) });
    if (c === 'str_replace') return step('assistant', 'edit', { tool: name, path, cmd: `str_replace ${path}`, text: str(a.new_str), aux: str(a.old_str) });
    if (c === 'insert') return step('assistant', 'edit', { tool: name, path, cmd: `insert ${path}:${a.insert_line ?? ''}`, text: str(a.new_str) });
    return step('assistant', 'edit', { tool: name, path, cmd: `${c} ${path}`, text: '' });   // undo_edit
  }
  if (SHELL.has(name)) { const c = str(a.command ?? a.cmd ?? a.code ?? ''); return step('assistant', 'command', { tool: name, cmd: c, text: c }); }
  if (READ.has(name)) return step('assistant', 'view', { tool: name, path, cmd: `${name} ${path}${a.offset != null ? ` @${a.offset}` : ''}${a.limit != null ? ` +${a.limit}` : ''}`, text: `${name} ${path}` });
  if (WRITE.has(name)) return step('assistant', 'edit', { tool: name, path, cmd: `${name} ${path}`, text: str(a.content ?? a.file_text ?? a.contents ?? '') });
  if (EDIT.has(name)) {
    const edits = Array.isArray(a.edits) ? a.edits : null;
    return step('assistant', 'edit', {
      tool: name, path, cmd: `${name} ${path}`,
      text: edits ? edits.map(e => str(e.new_string ?? e.new_str)).join('\n') : str(a.new_string ?? a.new_str ?? a.newString ?? a.content ?? ''),
      aux: edits ? edits.map(e => str(e.old_string ?? e.old_str)).join('\n') : str(a.old_string ?? a.old_str ?? a.oldString ?? ''),
    });
  }
  if (THINK.has(name)) return step('assistant', 'thought', { tool: name, text: str(a.thought ?? a.todos ?? a.task_list ?? a) });
  if (name === 'finish' || name === 'submit') return step('assistant', 'finish', { tool: name, cmd: name, text: str(a.message ?? a.outputs ?? '') });
  // Search, fetch, delegate and the rest: a call, with its arguments as the command.
  const c = `${name} ${short(a, 2000)}`;
  return step('assistant', 'command', { tool: name, cmd: c, text: c });
}

// ---- 1. OpenHands, tool-call messages (nebius SWE-rebench-openhands-trajectories) -----------
export function fromOpenHandsToolCalls(row) {
  const msgs = parse(row.trajectory, []);
  const steps = [], byCall = new Map();
  let task = '';
  for (const m of msgs) {
    const content = str(m.content);
    if (m.role === 'system') steps.push(step('system', 'message', { text: content }));
    else if (m.role === 'user') { if (!task) task = content; steps.push(step('user', 'message', { text: content })); }
    else if (m.role === 'assistant') {
      const calls = m.tool_calls || [];
      if (content.trim()) steps.push(step('assistant', calls.length ? 'thought' : 'message', { text: content }));
      for (const c of calls) {
        const s = toolCall(c.function?.name, parse(c.function?.arguments));
        if (c.id) byCall.set(c.id, steps.length);
        steps.push(s);
      }
    } else if (m.role === 'tool') {
      const call = byCall.get(m.tool_call_id) ?? null;
      const ec = content.match(/\[Command finished with exit code (-?\d+)\]/);
      steps.push(step('tool', 'output', { tool: str(m.name), text: content, call, exit_code: ec ? Number(ec[1]) : null }));
    }
  }
  return {
    meta: {
      native_id: row.trajectory_id, source: 'nebius-SWE-rebench-openhands-trajectories', harness: 'openhands', format: 'openhands tool-call messages',
      model: '', repo: str(row.repo), instance: str(row.instance_id), resolved: row.resolved == null ? null : Number(row.resolved) ? 1 : 0,
      exit_status: str(row.exit_status).startsWith('RuntimeError: Agent reached maximum iteration') ? 'max iterations' : str(row.exit_status),
      task, patch: str(row.model_patch), patch_from: row.model_patch ? 'dataset column model_patch' : '', feedback: '',
    },
    steps,
  };
}

// ---- 2. SWE-agent, tool-call format (SWE-smith-trajectories) --------------------------------
// The sample's `patch` column is not used: its patches do not belong to its trajectories (of
// 253, none equals the diff its own session printed at submit, and 6 name a file the session
// touched). The session's patch is the diff the harness printed when the agent submitted.
export function fromSweAgentToolCalls(row) {
  const msgs = parse(row.messages, []);
  const steps = [], byCall = new Map();
  let task = '', patch = '';
  for (const m of msgs) {
    const content = blocksText(m.content);
    if (m.role === 'system') steps.push(step('system', 'message', { text: content }));
    else if (m.role === 'user') { if (!task) task = content; steps.push(step('user', 'message', { text: content })); }
    else if (m.role === 'assistant') {
      const calls = m.tool_calls || [];
      const thought = str(m.thought ?? content);
      if (thought.trim()) steps.push(step('assistant', calls.length ? 'thought' : 'message', { text: thought }));
      for (const c of calls) {
        const s = toolCall(c.function?.name, parse(c.function?.arguments));
        if (c.id) byCall.set(c.id, steps.length);
        steps.push(s);
      }
    } else if (m.role === 'tool') {
      const id = (m.tool_call_ids || [])[0];
      const call = byCall.get(id) ?? null;
      const text = content.replace(/^OBSERVATION:\n/, '');
      // At submit the harness prints the working tree's diff for the agent to review.
      if (call != null && steps[call].kind === 'finish') { const d = text.match(/<diff>\n?([\s\S]*?)\n?<\/diff>/); if (d && d[1].includes('diff --git')) patch = d[1]; }
      steps.push(step('tool', 'output', { text, call }));
    }
  }
  const inst = str(row.instance_id);
  const repo = inst.split('.')[0].replace('__', '/');
  return {
    meta: {
      native_id: row.traj_id, source: 'SWE-smith-trajectories', harness: 'swe-agent', format: 'SWE-agent tool calls (thought + action)',
      model: str(row.model), repo, instance: inst, resolved: row.resolved == null ? null : row.resolved ? 1 : 0, exit_status: '',
      task, patch, patch_from: patch ? 'diff printed at submit' : '', feedback: '',
    },
    steps,
  };
}

// ---- 3. SWE-agent, text format (nebius SWE-agent-trajectories) ------------------------------
// The agent's command is the last fenced block of each `ai` message; the next `user` message is
// what it printed. The file an `edit` writes is the one the interface had open, which every
// observation names in its last lines: "(Open file: /repo/x.py)".
const SWEA_VIEW = new Set(['open', 'goto', 'scroll_down', 'scroll_up']);
export function fromSweAgentText(row, n) {
  const msgs = parse(row.trajectory, []);
  const steps = [];
  let task = '', openFile = '', lastCall = null;
  for (const m of msgs) {
    if (m.role === 'system') { steps.push(step('system', 'message', { text: str(m.system_prompt ?? m.text) })); continue; }
    const text = str(m.text);
    if (m.role === 'user') {
      const of = [...text.matchAll(/\(Open file: ([^)\n]+)\)/g)].at(-1);
      if (of) openFile = of[1].trim() === 'n/a' ? '' : of[1].trim();
      if (!task) { task = text; steps.push(step('user', 'message', { text })); continue; }
      // `open x.py` and `create x.py` name a file relative to the shell: the observation says where it is.
      const c = lastCall != null ? steps[lastCall] : null;
      if (c && openFile && (c.tool === 'open' || c.tool === 'create') && openFile.endsWith(c.path.replace(/^\.\//, ''))) { c.path = openFile; if (c.tool === 'create') c.cmd = `create ${openFile}`; }
      steps.push(step('tool', 'output', { text, call: lastCall }));
      lastCall = null;
      continue;
    }
    if (m.role !== 'ai') continue;
    const fences = [...text.matchAll(/```[a-zA-Z]*\n([\s\S]*?)\n?```/g)];
    const last = fences.at(-1);
    const thought = (last ? text.slice(0, last.index) : text).trim();
    if (thought) steps.push(step('assistant', last ? 'thought' : 'message', { text: thought }));
    if (!last) continue;
    const cmd = last[1].trim();
    const word = cmd.split(/\s+/)[0];
    let s;
    const edit = cmd.match(/^edit\s+(\d+):(\d+)\n([\s\S]*?)\n?end_of_edit\s*$/);
    if (edit) s = step('assistant', 'edit', { tool: 'edit', path: openFile, cmd: `edit ${edit[1]}:${edit[2]} ${openFile}`, text: edit[3] });
    else if (word === 'edit') s = step('assistant', 'edit', { tool: 'edit', path: openFile, cmd: `edit ${openFile}`, text: cmd.replace(/^edit[^\n]*\n?/, '').replace(/\n?end_of_edit\s*$/, '') });
    else if (word === 'create') { const p = cmd.split(/\s+/)[1] || ''; s = step('assistant', 'edit', { tool: 'create', path: p, cmd: `create ${p}`, text: '' }); }
    else if (SWEA_VIEW.has(word)) { const p = word === 'open' ? (cmd.split(/\s+/)[1] || '').replace(/^["']|["']$/g, '') : openFile; s = step('assistant', 'view', { tool: word, path: p, cmd, text: cmd }); }
    else if (word === 'submit') s = step('assistant', 'finish', { tool: 'submit', cmd: 'submit' });
    else s = step('assistant', 'command', { tool: 'bash', cmd, text: cmd });
    lastCall = steps.length;
    steps.push(s);
  }
  const inst = str(row.instance_id);
  return {
    meta: {
      native_id: `${inst}#${n}`, source: 'nebius-SWE-agent-trajectories', harness: 'swe-agent', format: 'SWE-agent text (fenced commands)',
      model: str(row.model_name), repo: inst.replace(/-\d+$/, '').replace('__', '/'), instance: inst, resolved: row.target == null ? null : row.target ? 1 : 0,
      exit_status: str(row.exit_status), task, patch: str(row.generated_patch), patch_from: row.generated_patch ? 'dataset column generated_patch' : '', feedback: '',
    },
    steps,
  };
}

// ---- 4. OpenHands, event stream (OpenHands-feedback) ----------------------------------------
// Runtime status lines and agent-state changes are dropped (and counted); screenshots and DOM
// dumps of browser observations are left out of the text.
export function fromOpenHandsEvents(row, n) {
  const events = parse(row.trajectory, []);
  const steps = [];
  let task = '', model = '', dropped = 0, lastCall = null;
  for (const e of events) {
    const x = parse(e.extras);
    const a = e.action, o = e.observation;
    if (a === 'initialize') { model ||= str(x.LLM_MODEL); dropped++; continue; }
    if (a === 'change_agent_state' || o === 'agent_state_changed') { dropped++; continue; }
    if (!a && !o) { dropped++; continue; }                          // STATUS$..., "ok", "No observation"
    if (a === 'message') {
      const text = str(e.content ?? x.content);
      if (e.source === 'user') { if (!task) task = text; steps.push(step('user', 'message', { text })); }
      else steps.push(step('assistant', 'message', { text }));
      continue;
    }
    if (a) {
      const thought = str(x.thought).trim();
      if (thought && a !== 'finish') steps.push(step(e.source === 'user' ? 'user' : 'assistant', 'thought', { text: thought }));
      let s;
      if (a === 'run') s = step('assistant', 'command', { tool: 'run', cmd: str(x.command), text: str(x.command) });
      else if (a === 'run_ipython') s = step('assistant', 'command', { tool: 'run_ipython', cmd: str(x.code), text: str(x.code) });
      else if (a === 'browse_interactive' || a === 'browse') s = step('assistant', 'command', { tool: 'browse', cmd: str(x.browser_actions ?? x.url), text: str(x.browser_actions ?? x.url) });
      else if (a === 'edit') s = step('assistant', 'edit', { tool: 'edit', path: str(x.path), cmd: `edit ${str(x.path)}`, text: str(x.content) });
      else if (a === 'finish') s = step('assistant', 'finish', { tool: 'finish', cmd: 'finish', text: thought || str(x.outputs?.content ?? '') });
      else { const c = `${a} ${short(x.inputs ?? x.query ?? x.plan ?? x, 2000)}`; s = step('assistant', 'command', { tool: a, cmd: c, text: c }); }
      if (e.source === 'user') s.role = 'user';                    // a command the user typed in the terminal
      lastCall = s.kind === 'finish' ? null : steps.length;
      steps.push(s);
      continue;
    }
    // an observation
    let text = str(e.content);
    if (o === 'browse') text = `${str(x.url)}\n${str(x.last_browser_action_error)}\n${text}`.trim();
    if (o === 'delegate') text = short(x.outputs, 20000);
    if (!text.trim()) text = str(e.message);
    steps.push(step('tool', 'output', { tool: o, text, call: lastCall, exit_code: typeof x.exit_code === 'number' ? x.exit_code : null }));
    lastCall = null;
  }
  return {
    meta: {
      native_id: `feedback#${n}`, source: 'OpenHands-feedback', harness: 'openhands', format: 'OpenHands event stream',
      model, repo: '', instance: '', resolved: null, exit_status: '', task, patch: '', feedback: str(row.feedback),
      started_at: str(row.timestamp), dropped,
    },
    steps,
  };
}

// ---- 5. Claude Code session JSONL (trace-commons; Swival exports the same shape) ------------
export function fromClaudeCodeJsonl(lines, { native_id, source, harness = 'claude-code', format = 'Claude Code session JSONL' }) {
  const steps = [], byCall = new Map();
  let task = '', model = '', repo = '', dropped = 0, started = '';
  for (const l of lines) {
    if (l.type !== 'user' && l.type !== 'assistant') { dropped++; continue; }   // snapshots, titles, queue operations
    started ||= str(l.timestamp);
    repo ||= str(l.cwd);
    const c = l.message?.content;
    if (l.type === 'user') {
      if (typeof c === 'string') { if (!l.isMeta) { if (!task) task = c; steps.push(step('user', 'message', { text: c })); } else dropped++; continue; }
      for (const b of c || []) {
        if (b.type === 'tool_result') steps.push(step('tool', 'output', { text: blocksText(b.content), call: byCall.get(b.tool_use_id) ?? null, exit_code: b.is_error ? 1 : null }));
        else if (b.type === 'text') { if (!l.isMeta) { if (!task) task = b.text; steps.push(step('user', 'message', { text: b.text })); } else dropped++; }
      }
      continue;
    }
    model ||= str(l.message?.model);
    const blocks = Array.isArray(c) ? c : [{ type: 'text', text: str(c) }];
    const calls = blocks.some(b => b.type === 'tool_use');
    for (const b of blocks) {
      if (b.type === 'thinking') { if (str(b.thinking).trim()) steps.push(step('assistant', 'thought', { text: b.thinking })); }
      else if (b.type === 'text') { if (str(b.text).trim()) steps.push(step('assistant', calls ? 'thought' : 'message', { text: b.text })); }
      else if (b.type === 'tool_use') { if (b.id) byCall.set(b.id, steps.length); steps.push(toolCall(b.name, b.input)); }
    }
  }
  return { meta: { native_id, source, harness, format, model, repo: repo.split(/[\\/]/).pop() || '', instance: '', resolved: null, exit_status: '', task, patch: '', feedback: '', started_at: started, dropped }, steps };
}

// ---- 6. Cursor agent transcript JSONL (trace-commons): no tool results are recorded ---------
export function fromCursorJsonl(lines, { native_id, source }) {
  const steps = [];
  let task = '', dropped = 0;
  for (const l of lines) {
    const blocks = l.message?.content;
    if (!l.role || !Array.isArray(blocks)) { dropped++; continue; }
    const calls = blocks.some(b => b.type === 'tool_use');
    for (const b of blocks) {
      if (b.type === 'text') {
        const text = str(b.text);
        if (!text.trim()) continue;
        if (l.role === 'user') { if (!task) task = text.replace(/<\/?user_query>/g, '').trim(); steps.push(step('user', 'message', { text })); }
        else steps.push(step('assistant', calls ? 'thought' : 'message', { text }));
      } else if (b.type === 'tool_use') steps.push(toolCall(b.name, b.input));
    }
  }
  return { meta: { native_id, source, harness: 'cursor', format: 'Cursor agent transcript JSONL', model: '', repo: '', instance: '', resolved: null, exit_status: '', task, patch: '', feedback: '', dropped }, steps };
}

// ---- 7. opencode session JSON (trace-commons) ------------------------------------------------
export function fromOpencodeJson(doc, { native_id, source }) {
  const steps = [];
  let task = '', dropped = 0;
  for (const m of doc.messages || []) {
    const role = m.info?.role === 'user' ? 'user' : 'assistant';
    const calls = (m.parts || []).some(p => p.type === 'tool');
    for (const p of m.parts || []) {
      if (p.type === 'text') {
        if (!str(p.text).trim()) continue;
        if (role === 'user') { if (!task) task = p.text; steps.push(step('user', 'message', { text: p.text })); }
        else steps.push(step('assistant', calls ? 'thought' : 'message', { text: p.text }));
      } else if (p.type === 'reasoning') { if (str(p.text).trim()) steps.push(step('assistant', 'thought', { text: p.text })); }
      else if (p.type === 'tool') {
        const at = steps.length;
        steps.push(toolCall(p.tool, p.state?.input));
        if (p.state?.output != null || p.state?.error != null) steps.push(step('tool', 'output', { tool: p.tool, text: str(p.state.output ?? p.state.error), call: at, exit_code: p.state?.status === 'error' ? 1 : null }));
      } else dropped++;                                              // step-start, step-finish
    }
  }
  const info = doc.info || {};
  return {
    meta: {
      native_id, source, harness: 'opencode', format: 'opencode session JSON', model: str(info.model?.id), repo: '', instance: '', resolved: null, exit_status: '',
      task, patch: '', feedback: '', started_at: info.time?.created ? new Date(info.time.created).toISOString() : '', dropped,
    },
    steps,
  };
}

// ---- what a command or an output is, read from its text ------------------------------------
const FLAG_RULES = [
  ['test', /\b(pytest|py\.test|unittest|nosetests|tox|runtests\.py|manage\.py test|npm (run )?test|yarn test|go test|cargo test|make (test|check)|ctest|rspec|phpunit|mvn test|gradle test)\b/],
  ['install', /\b(pip3?|python3? -m pip|uv pip|conda|mamba|npm|yarn|pnpm|apt-get|apt|brew|gem|cargo|go) +(install|add|get)\b|%pip install/],
  ['delete', /\brm +(-[a-zA-Z]+ +)*\S|\bgit +(clean|reset +--hard)\b|\bshred\b/],
  ['network', /\b(curl|wget|ssh|scp|rsync|nc|telnet|ftp)\b|\bgit +(clone|push|pull|fetch)\b|https?:\/\//],
  ['git', /\bgit +\w/],
  ['python', /\bpython3?\b/],
];
const ERR = /Traceback \(most recent call last\)|(^|\n)(FAILED|ERROR) |=+ (FAILURES|ERRORS) =+|\b\d+ (failed|errors?)\b|\b[A-Z]\w*(Error|Exception): |command not found|No such file or directory|Permission denied|\[Command finished with exit code [1-9]|SyntaxError|Your proposed edit has introduced new syntax error/;
const TESTISH = /(^|\/)(tests?|testing|spec)\/|(^|\/)test_[^/]*$|_test\.[a-z]+$|\.test\.[a-z]+$|(^|\/)conftest\.py$/;

export function flagsOf(s) {
  const out = [];
  if (s.kind === 'command') for (const [name, re] of FLAG_RULES) if (re.test(s.cmd)) out.push(name);
  if ((s.kind === 'edit' || s.kind === 'view') && TESTISH.test(s.path)) out.push('test-file');
  return out;
}

function clip(text, cap) {
  if (text.length <= cap) return text;
  const head = Math.floor(cap * 0.75), tail = cap - head;
  return `${text.slice(0, head)}\n…[${text.length - cap} characters clipped at ingest; the full text's SHA-256 is in raw_sha]…\n${text.slice(text.length - tail)}`;
}

// The text of a step's hash: its fields in a fixed order. verify recomputes it from the row.
export function canonical(s) {
  return JSON.stringify([s.id, s.role, s.kind, s.tool || '', s.cmd || '', s.path || '', s.text || '', s.aux || '', s.out_bytes || 0, s.raw_sha || '']);
}
export const genesis = sessionId => sha(`${CHAIN_VERSION}:${sessionId}`);
export const link = (prev, s) => sha(`${prev}\n${canonical(s)}`);
// The root over every session's head hash: the one value to keep somewhere else.
export const ledgerRoot = heads => sha(heads.map(h => `${h.id}:${h.head_hash}`).sort().join('\n'));

// ---- ids, flags, edges and the hash chain ---------------------------------------------------
export function finalise(sessionId, { meta, steps }) {
  const rows = [];
  let prev = genesis(sessionId);
  let lastThought = -1, lastOutput = -1, lastFail = -1;
  const views = new Map();          // path -> seqs of its views
  const outputOf = new Map();       // call seq -> output seq
  steps.forEach((s, seq) => { if (s.kind === 'output' && s.call != null && !outputOf.has(s.call)) outputOf.set(s.call, seq); });
  const id = seq => `${sessionId}:${seq}`;
  steps.forEach((s, seq) => {
    const full = clean(s.text);
    const text = clip(full, s.kind === 'edit' ? EDIT_CAP : TEXT_CAP);
    const cmd = clip(clean(s.cmd), TEXT_CAP);
    const row = {
      id: id(seq), session: sessionId, seq, role: s.role, kind: s.kind, tool: clean(s.tool), cmd, path: clean(s.path),
      text: s.kind === 'command' ? cmd : text, aux: clip(clean(s.aux), EDIT_CAP),
      out_bytes: Buffer.byteLength(full), raw_sha: text === full ? '' : sha(full),
      harness: meta.harness, source: meta.source, resolved: meta.resolved,
      exit_code: s.exit_code, err: 0, cmd_sig: '', call: s.kind === 'output' && s.call != null ? id(s.call) : '', flags: [], links: [],
    };
    if (s.kind === 'command') row.cmd_sig = sha(cmd.trim()).slice(0, 16);
    if (s.kind === 'output') row.err = (s.exit_code != null && s.exit_code !== 0) || ERR.test(full.length > 60_000 ? full.slice(0, 30_000) + full.slice(-30_000) : full) ? 1 : 0;
    row.flags = flagsOf(row);
    // edges
    if (outputOf.has(seq)) row.links.push(['output', id(outputOf.get(seq))]);
    if (s.kind === 'thought') { if (lastOutput >= 0) row.links.push(['saw', id(lastOutput)]); }
    else if (lastThought >= 0 && s.role !== 'system' && s.role !== 'user') row.links.push(['thought', id(lastThought)]);
    if (s.kind === 'edit') {
      for (const v of (views.get(row.path) || []).slice(-3).reverse()) row.links.push(['view', id(v)]);
      if (lastFail >= 0) row.links.push(['failed', id(lastFail)]);
    }
    if (s.kind === 'view' && row.path) { if (!views.has(row.path)) views.set(row.path, []); views.get(row.path).push(seq); }
    if (s.kind === 'thought') lastThought = seq;
    if (s.kind === 'output') { lastOutput = seq; if (row.err) lastFail = seq; }
    row.prev_hash = prev;
    row.hash = link(prev, row);
    prev = row.hash;
    rows.push(row);
  });
  const count = k => rows.filter(r => r.kind === k).length;
  const session = {
    id: sessionId, native_id: clean(meta.native_id), source: meta.source, harness: meta.harness, format: meta.format, model: clean(meta.model), repo: clean(meta.repo),
    instance: clean(meta.instance), resolved: meta.resolved, exit_status: clean(meta.exit_status), feedback: meta.feedback || '',
    started_at: meta.started_at || '', steps: rows.length, n_thought: count('thought'), n_command: count('command'), n_edit: count('edit'), n_view: count('view'),
    n_output: count('output'), n_err: rows.filter(r => r.err).length, dropped: meta.dropped || 0,
    bytes: rows.reduce((a, r) => a + r.out_bytes, 0), task: clip(clean(meta.task), TEXT_CAP), patch: clean(meta.patch), patch_from: meta.patch_from || '', title: '',
    head_hash: prev,
  };
  return { session, steps: rows };
}

// ---- blame: every added line of the final patch, and the call that wrote it -----------------
// A line is traced to an edit call when that call wrote the same file (its path ends with the
// patch's path) and the content it wrote (new_str, file_text, an edit's replacement) holds the
// line exactly. Several calls can hold it: the one that also holds the patch's neighbouring
// added lines is preferred, then the latest. A line no edit call holds is looked for in the
// text of shell commands (a heredoc, sed, echo): that is reported apart, as "shell". Blank
// lines are not counted either way.
export function parsePatch(patch) {
  const files = [];
  let cur = null, newNo = 0, hunk = 0;
  for (const raw of patch.split('\n')) {
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    if (line.startsWith('diff --git ')) { cur = { path: (line.match(/ b\/(.+)$/) || [])[1] || '', added: [] }; files.push(cur); hunk = 0; continue; }
    if (!cur) continue;
    if (line.startsWith('+++ ')) { const p = line.slice(4).trim(); if (p !== '/dev/null') cur.path = p.replace(/^b\//, ''); continue; }
    if (line.startsWith('--- ')) continue;
    const h = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (h) { newNo = Number(h[1]); hunk++; continue; }
    if (!hunk) continue;
    if (line.startsWith('+')) { cur.added.push({ n: newNo, hunk, text: line.slice(1) }); newNo++; }
    else if (line.startsWith('-') || line.startsWith('\\')) { /* a removed line, or "No newline at end of file" */ }
    else newNo++;
  }
  return files.filter(f => f.added.length);
}

export function blame(session, steps) {
  const files = parsePatch(session.patch || '');
  if (!files.length) return [];
  const edits = steps.filter(s => s.kind === 'edit' && s.text).map(s => ({ s, lines: new Set(s.text.split('\n').map(l => l.replace(/\r$/, ''))) }));
  const shells = steps.filter(s => s.kind === 'command' && s.cmd.length > 20);
  const viewsOf = new Map();
  for (const s of steps) if (s.kind === 'view' && s.path) { if (!viewsOf.has(s.path)) viewsOf.set(s.path, []); viewsOf.get(s.path).push(s.seq); }
  const samePath = (p, f) => p === f || p.endsWith('/' + f) || p.replace(/\\/g, '/').endsWith('/' + f);
  const out = [];
  let k = 0;
  for (const f of files) {
    const mine = edits.filter(e => samePath(e.s.path, f.path));
    f.added.forEach((a, i) => {
      const row = { id: `${session.id}:${k}`, session: session.id, k, file: f.path, n: a.n, hunk: a.hunk, text: a.text, via: 'none', step: '', origin: '', back: -1, harness: session.harness, source: session.source };
      k++;
      out.push(row);
      if (!a.text.trim()) { row.via = 'blank'; return; }
      const cands = mine.filter(e => e.lines.has(a.text));
      if (cands.length) {
        const prev = f.added[i - 1], next = f.added[i + 1];
        const score = e => (prev && prev.hunk === a.hunk && e.lines.has(prev.text) ? 1 : 0) + (next && next.hunk === a.hunk && e.lines.has(next.text) ? 1 : 0);
        const best = cands.reduce((b, e) => (score(e) > score(b) || (score(e) === score(b) && e.s.seq > b.s.seq) ? e : b));
        row.via = 'edit'; row.step = best.s.id;
        // the observation the edit rests on: the agent's latest view of that file before it
        const v = (viewsOf.get(best.s.path) || []).filter(q => q < best.s.seq).at(-1);
        if (v != null) { row.origin = `${session.id}:${v}`; row.back = best.s.seq - v; }
        return;
      }
      const t = a.text.trim();
      if (t.length >= 8) {
        const sh = shells.filter(s => s.cmd.includes(t)).at(-1);
        if (sh) { row.via = 'shell'; row.step = sh.id; }
      }
    });
  }
  return out;
}

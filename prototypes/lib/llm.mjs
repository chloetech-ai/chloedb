// Claude for the prototypes, behind a spend cap.
//
//   import { complete, runTools, textOf, spendStatus } from '../../lib/llm.mjs';
//   const r = await complete({ app: 'leaklab', ip, system, messages, max_tokens: 2000 });
//   textOf(r) -> the answer's text; r.cost_usd -> what this call cost
//
// Every call is priced from its usage and counted: the site's live calls against a daily
// cap (PROTOS_LLM_DAILY_USD, default 25), an offline script's against its run cap
// (PROTOS_LLM_RUN_USD, default 10), everything against a total cap (PROTOS_LLM_TOTAL_USD,
// default 150), and each visitor's address against an hourly count
// (PROTOS_LLM_PER_IP_HOUR, default 40). Past a cap, calls throw LlmError with code
// 'budget' or 'rate' before anything is sent, so an app shows its recorded results instead.
// The spend is kept in ChloeDB (table protos_spend, a row per day and process), so a
// restart does not reset it; a script calls flushSpend() before it exits. Identical calls
// with `cache: true` are answered from memory.
import Anthropic from '@anthropic-ai/sdk';
import { createHash } from 'node:crypto';
import * as db from './db.mjs';

export const DEFAULT_MODEL = process.env.PROTOS_MODEL || 'claude-opus-5-5';
export const CHEAP_MODEL = process.env.PROTOS_CHEAP_MODEL || 'claude-haiku-5-5';

// $ per million tokens: [input, output]. Haiku's price is for prompts up to 100K tokens.
const PRICES = {
  'claude-haiku-5-5': [0.10, 0.50],
  'claude-sonnet-5-5': [2, 10],
  'claude-opus-5-5': [4, 20],
  'claude-fable-5-2': [8, 40],
};
const FALLBACK_MODELS = new Set(['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-fable-5-2']);

const DAILY_USD = Number(process.env.PROTOS_LLM_DAILY_USD || 25);
const TOTAL_USD = Number(process.env.PROTOS_LLM_TOTAL_USD || 150);
const PER_IP_HOUR = Number(process.env.PROTOS_LLM_PER_IP_HOUR || 40);
const SPEND_TABLE = 'protos_spend';

export class LlmError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}

let client = null;
function anthropic() {
  if (!client) {
    if (!process.env.ANTHROPIC_API_KEY) throw new LlmError('no_key', 'no ANTHROPIC_API_KEY in the environment');
    client = new Anthropic({ maxRetries: 2, timeout: 120_000 });
  }
  return client;
}

export function priceOf(model) { return PRICES[model] || PRICES['claude-opus-5-5']; }

export function costOf(model, usage = {}) {
  const [pin, pout] = priceOf(model);
  const inTok = usage.input_tokens || 0;
  const cacheWrite = usage.cache_creation_input_tokens || 0;
  const cacheRead = usage.cache_read_input_tokens || 0;
  const out = usage.output_tokens || 0;
  return (inTok * pin + cacheWrite * pin * 1.25 + cacheRead * pin * 0.1 + out * pout) / 1e6;
}

// ---- spend, kept in ChloeDB -------------------------------------------------------
// One row per day and process: the site ('site') and each offline measurement script
// ('script:<name>') count apart, so they never overwrite each other. The daily cap applies
// to the site's live traffic; a script stops at PROTOS_LLM_RUN_USD (default 10) for its
// run; everything together stops at the total cap.
const RUN_USD = Number(process.env.PROTOS_LLM_RUN_USD || 10);
const PROC = process.env.PROTOS_PROC || ((process.argv[1] || '').endsWith('server.mjs') ? 'site' : `script:${(process.argv[1] || 'node').split('/').pop()}`);
const today = () => new Date().toISOString().slice(0, 10);
const spend = { loaded: false, day: today(), usd: 0, calls: 0, byApp: {}, totalUsd: 0, runUsd: 0, othersTotal: 0, refreshedAt: 0 };
let saveTimer = null;
const rowId = () => `${spend.day}|${PROC}`;

async function refreshTotals() {
  const r = await db.query(`SELECT id, usd, calls, by_app FROM ${SPEND_TABLE} LIMIT 10000`);
  let others = 0;
  for (const row of r.rows) {
    if (row.id === rowId()) {
      if (!spend.loaded) {
        spend.usd = row.usd || 0;
        spend.calls = row.calls || 0;
        try { spend.byApp = JSON.parse(row.by_app || '{}'); } catch { spend.byApp = {}; }
      }
    } else others += row.usd || 0;
  }
  spend.othersTotal = others;
  spend.totalUsd = others + spend.usd;
  spend.refreshedAt = Date.now();
}

async function loadSpend() {
  if (spend.loaded && Date.now() - spend.refreshedAt < 60_000) return;
  try {
    if (!spend.loaded) await db.ensureTable(SPEND_TABLE, `CREATE TABLE ${SPEND_TABLE} (id KEYWORD PRIMARY KEY, usd FLOAT, calls INT, by_app KEYWORD)`);
    await refreshTotals();
  } catch (e) {
    if (!spend.loaded) console.error('[llm] could not read spend from ChloeDB; counting in memory:', e.message);
    spend.refreshedAt = Date.now();
  }
  spend.loaded = true;
}

async function saveNow() {
  try {
    await db.write([{ op: 'upsert', table: SPEND_TABLE, doc: { id: rowId(), usd: spend.usd, calls: spend.calls, by_app: JSON.stringify(spend.byApp) } }]);
  } catch (e) { console.error('[llm] could not save spend:', e.message); }
}

function scheduleSave() {
  if (saveTimer) return;
  saveTimer = setTimeout(() => { saveTimer = null; saveNow(); }, 1000);
}

// Writes the spend row now (a script calls it before it exits).
export async function flushSpend() {
  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
  await saveNow();
}

function rollDay() {
  const d = today();
  if (d !== spend.day) { spend.othersTotal += spend.usd; spend.day = d; spend.usd = 0; spend.calls = 0; spend.byApp = {}; }
}

function record(app, usd) {
  rollDay();
  spend.usd += usd; spend.totalUsd += usd; spend.runUsd += usd; spend.calls += 1;
  const a = spend.byApp[app] || (spend.byApp[app] = { usd: 0, calls: 0 });
  a.usd = +(a.usd + usd).toFixed(6); a.calls += 1;
  scheduleSave();
}

function budgetCheck() {
  if (PROC === 'site' && spend.usd >= DAILY_USD) throw new LlmError('budget', `today's live model budget ($${DAILY_USD}) is spent; showing recorded results`);
  if (spend.totalUsd >= TOTAL_USD) throw new LlmError('budget', `the model budget for these prototypes ($${TOTAL_USD}) is spent; showing recorded results`);
  if (PROC !== 'site' && spend.runUsd >= RUN_USD) throw new LlmError('budget', `this run reached its budget ($${RUN_USD}, PROTOS_LLM_RUN_USD)`);
}

export async function spendStatus() {
  await loadSpend();
  rollDay();
  return {
    day: spend.day, process: PROC, usd: +spend.usd.toFixed(4), calls: spend.calls, daily_cap_usd: DAILY_USD,
    total_usd: +spend.totalUsd.toFixed(4), total_cap_usd: TOTAL_USD, by_app: spend.byApp,
    live: Boolean(process.env.ANTHROPIC_API_KEY) && spend.usd < DAILY_USD && spend.totalUsd < TOTAL_USD,
  };
}

// ---- per-visitor rate limit -------------------------------------------------------
const hits = new Map(); // ip -> [timestamps]
function rateCheck(ip, weight = 1) {
  if (!ip) return;
  const now = Date.now();
  const list = (hits.get(ip) || []).filter(t => now - t < 3600_000);
  if (list.length + weight > PER_IP_HOUR) {
    hits.set(ip, list);
    throw new LlmError('rate', `this address made ${list.length} live model calls in the last hour (limit ${PER_IP_HOUR}); showing recorded results`);
  }
  for (let i = 0; i < weight; i++) list.push(now);
  hits.set(ip, list);
  if (hits.size > 50_000) hits.clear();
}

export function rateRemaining(ip) {
  const now = Date.now();
  const list = (hits.get(ip) || []).filter(t => now - t < 3600_000);
  return Math.max(0, PER_IP_HOUR - list.length);
}

// ---- answers kept in memory ---------------------------------------------------------
const memo = new Map();
function memoGet(k) { const v = memo.get(k); if (v) { memo.delete(k); memo.set(k, v); } return v; }
function memoSet(k, v) { memo.set(k, v); if (memo.size > 2000) memo.delete(memo.keys().next().value); }

// ---- calls ----------------------------------------------------------------------------
// complete(): one Messages API call. `app` tags the spend; `ip` applies the visitor
// limit (leave it out for offline measurement runs). `effort` is low by default to keep
// demos fast. Opus, Sonnet and Fable calls carry server-side refusal fallbacks unless
// `fallback: false`. Returns the API response plus cost_usd and from_cache.
export async function complete({
  app = 'unknown', ip = null, model = DEFAULT_MODEL, system, messages, tools, tool_choice,
  max_tokens = 4000, effort = 'low', cache = false, fallback = true, extra = {},
}) {
  await loadSpend();
  rollDay();
  const key = cache ? createHash('sha256').update(JSON.stringify({ model, system, messages, tools, max_tokens, effort })).digest('hex') : null;
  if (key) {
    const hit = memoGet(key);
    if (hit) return { ...hit, cost_usd: 0, from_cache: true };
  }
  budgetCheck();
  rateCheck(ip);

  const params = {
    model, max_tokens, messages,
    ...(system ? { system } : {}),
    ...(tools ? { tools } : {}),
    ...(tool_choice ? { tool_choice } : {}),
    output_config: { effort },
    ...extra,
  };
  let resp;
  try {
    if (fallback && FALLBACK_MODELS.has(model)) {
      resp = await anthropic().beta.messages.create({ ...params, betas: ['server-side-fallback-2026-07-01'], fallbacks: 'default' });
    } else {
      resp = await anthropic().messages.create(params);
    }
  } catch (e) {
    if (e instanceof Anthropic.RateLimitError) throw new LlmError('api_rate', 'the model API is busy (429); try again in a minute');
    if (e instanceof Anthropic.APIError) throw new LlmError('api', `model API error ${e.status}: ${e.message}`);
    throw e;
  }
  const usd = costOf(resp.model || model, resp.usage);
  record(app, usd);
  const out = { ...resp, cost_usd: usd, from_cache: false };
  if (key && resp.stop_reason !== 'refusal') memoSet(key, out);
  return out;
}

export function textOf(resp) {
  return (resp.content || []).filter(b => b.type === 'text').map(b => b.text).join('');
}

// Parses the first JSON object or array in a model's text answer.
export function jsonOf(resp) {
  const t = typeof resp === 'string' ? resp : textOf(resp);
  const m = t.match(/```(?:json)?\s*([\s\S]*?)```/);
  const s = m ? m[1] : t;
  const i = Math.min(...['{', '['].map(c => { const k = s.indexOf(c); return k < 0 ? Infinity : k; }));
  if (!Number.isFinite(i)) throw new Error('no JSON in the answer');
  const open = s[i], close = open === '{' ? '}' : ']';
  const j = s.lastIndexOf(close);
  return JSON.parse(s.slice(i, j + 1));
}

// runTools(): a tool-use loop. `handlers` maps a tool's name to an async function of its
// input that returns a string (or an object, sent as JSON). `onStep` sees each step as it
// happens: {kind:'assistant', content, cost_usd} and {kind:'tool', name, input, output}.
// Stops at end_turn, refusal, maxTurns or when a handler throws StopLoop.
export class StopLoop extends Error {}

export async function runTools({ handlers = {}, maxTurns = 8, onStep, messages, ...rest }) {
  const convo = [...messages];
  let last = null, turns = 0, cost = 0;
  while (turns < maxTurns) {
    turns += 1;
    last = await complete({ ...rest, messages: convo });
    cost += last.cost_usd;
    convo.push({ role: 'assistant', content: last.content });
    if (onStep) await onStep({ kind: 'assistant', content: last.content, stop_reason: last.stop_reason, cost_usd: last.cost_usd, turn: turns });
    if (last.stop_reason !== 'tool_use') break;
    const results = [];
    let stop = false;
    for (const block of last.content.filter(b => b.type === 'tool_use')) {
      let output, isError = false;
      try {
        const h = handlers[block.name];
        if (!h) throw new Error(`no such tool: ${block.name}`);
        output = await h(block.input || {}, block);
      } catch (e) {
        if (e instanceof StopLoop) { stop = true; output = e.message || 'stopped'; }
        else { isError = true; output = `error: ${e.message}`; }
      }
      const content = typeof output === 'string' ? output : JSON.stringify(output);
      results.push({ type: 'tool_result', tool_use_id: block.id, content, ...(isError ? { is_error: true } : {}) });
      if (onStep) await onStep({ kind: 'tool', name: block.name, input: block.input, output: content, is_error: isError, turn: turns });
    }
    convo.push({ role: 'user', content: results });
    if (stop) break;
  }
  return { messages: convo, last, turns, cost_usd: cost, refused: last?.stop_reason === 'refusal' };
}

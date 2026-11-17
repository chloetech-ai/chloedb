// MCP Atlas — measure: the one model pass. Classify each secret-bearing server by what its
// description says it can DO with the credential it asks for (read/write/execute/send/pay/
// unclear), with claude-haiku-5-5 through lib/llm.mjs (spend-capped). Writes `capability`
// onto atlas_servers, records the labels in data/atlas-capability.jsonl (which load.mjs
// reads) and refreshes atlas_numbers (now including the model rows).
//
//   ANTHROPIC_API_KEY=... CHLOEDB_URL=... node apps/atlas/measure.mjs
//   ATLAS_MEASURE_LIMIT=500 node apps/atlas/measure.mjs   # a small proof run
//
// In ChloeDB's own SQL this is:
//   SELECT name, llm('anthropic/claude-haiku-5-5',
//     'What can this server do with the credential it asks for?', description,
//     choices => 'read,write,execute,send,pay,unclear') AS can
//   FROM atlas_servers WHERE has_secret = 1;
// We run the same classification through lib/llm.mjs so it counts against the shared spend
// cap (every model call of the prototypes goes there); the label is the model's, batched.
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';
import { complete, textOf, flushSpend, spendStatus, LlmError, CHEAP_MODEL } from '../../lib/llm.mjs';
import { writeNumbers } from './lib/numbers.mjs';

const BATCH = Number(process.env.ATLAS_BATCH || 25);
const LIMIT = Number(process.env.ATLAS_MEASURE_LIMIT || 0);
const CHOICES = ['read', 'write', 'execute', 'send', 'pay', 'unclear'];
const LABELS = path.join(path.dirname(fileURLToPath(import.meta.url)), 'data', 'atlas-capability.jsonl');

// The labels, recorded beside the prep artifact, so that load.mjs fills a fresh database with
// them (and a second load keeps them) without another model pass. One query per label: the
// server answers at most 10,000 rows, and a KEYWORD has no range to page by.
async function exportLabels() {
  const out = [];
  for (const c of CHOICES) {
    const r = await db.query(`SELECT name FROM atlas_servers WHERE capability = :c LIMIT 10000`, { c });
    if (r.rows.length >= 10000) throw new Error(`label ${c} has 10,000 rows or more: page it before exporting`);
    for (const x of r.rows) out.push({ name: x.name, capability: c });
  }
  out.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  writeFileSync(LABELS, out.map(x => JSON.stringify(x)).join('\n') + '\n');
  return out.length;
}

const SYS = `You label MCP (Model Context Protocol) servers by what the server can DO with the credential it asks for, judging only from the title and description. For each numbered server, answer with exactly one label:
- read: only reads or searches data
- write: creates, edits or deletes data
- execute: runs code, commands or browser/computer actions
- send: sends messages or email, or posts to a channel
- pay: moves money, signs transactions, or touches a wallet/payment
- unclear: the description does not say
Reply with ONLY a JSON array of strings, one label per server in order, nothing else.`;

function parseLabels(text, n) {
  let arr = null;
  const m = text.match(/\[[\s\S]*\]/);
  if (m) { try { arr = JSON.parse(m[0]); } catch {} }
  if (!Array.isArray(arr)) arr = text.split(/[\n,]+/).map(s => s.replace(/[^a-z]/gi, '').toLowerCase());
  const out = [];
  for (let i = 0; i < n; i++) { let v = String(arr[i] || '').toLowerCase().replace(/[^a-z]/g, ''); out.push(CHOICES.includes(v) ? v : 'unclear'); }
  return out;
}

async function run() {
  await db.health();
  const secret = (await db.query(`SELECT COUNT(*) AS v FROM atlas_servers WHERE has_secret = 1`)).rows[0].v;
  const already0 = (await db.query(`SELECT COUNT(*) AS v FROM atlas_servers WHERE has_secret = 1 AND capability IS NOT NULL AND capability <> ''`)).rows[0].v;
  console.log(`[measure] ${secret} secret-bearing servers; ${already0} already classified`);

  let done = 0, failed = 0, cost = 0, budgetHit = false;
  // Fetch unclassified servers a page at a time (the server returns at most 10,000 rows, and
  // the WHERE shrinks as we write capabilities). Stop at the run budget or ATLAS_MEASURE_LIMIT.
  outer: for (;;) {
    const r = await db.query(`SELECT name, title, description FROM atlas_servers WHERE has_secret = 1 AND (capability IS NULL OR capability = '') ORDER BY name LIMIT 2000`);
    const todo = r.rows;
    if (!todo.length) break;
    for (let i = 0; i < todo.length; i += BATCH) {
      if (LIMIT && done >= LIMIT) break outer;
      const batch = todo.slice(i, i + BATCH);
      const prompt = batch.map((s, k) => `${k + 1}. ${s.title || s.name}: ${(s.description || '').slice(0, 220)}`).join('\n');
      let labels;
      try {
        const resp = await complete({ app: 'atlas', model: CHEAP_MODEL, max_tokens: 300, effort: 'low',
          system: SYS, messages: [{ role: 'user', content: prompt }] });
        cost += resp.cost_usd || 0;
        labels = parseLabels(textOf(resp), batch.length);
      } catch (e) {
        if (e instanceof LlmError && e.code === 'budget') { console.log(`[measure] budget reached: ${e.message}`); budgetHit = true; break outer; }
        failed += batch.length; console.error(`[measure] batch failed: ${e.message}`); continue;
      }
      const vals = batch.map((s, k) => `(:n${k}, :c${k})`).join(',');
      const params = {};
      batch.forEach((s, k) => { params[`n${k}`] = s.name; params[`c${k}`] = labels[k]; });
      await db.sql(`INSERT INTO atlas_servers (name, capability) VALUES ${vals} ON CONFLICT (name) DO UPDATE SET capability = excluded.capability`, params);
      done += batch.length;
      if (done % 500 === 0) console.log(`  …${done} classified  $${cost.toFixed(3)}`);
    }
  }
  await flushSpend();
  if (budgetHit) console.log('[measure] stopped at budget; remaining servers keep their recorded (empty) capability');
  console.log(`[measure] classified ${done}, failed ${failed}, cost $${cost.toFixed(3)}`);
  console.log(`[measure] exported ${await exportLabels()} labels to ${path.relative(process.cwd(), LABELS)}`);
  const n = await writeNumbers(db, { withModel: true });
  console.log(`[measure] wrote ${n} number rows (including model rows)`);
  const s = await spendStatus();
  console.log(`[measure] spend so far: $${s.total_usd} of $${s.total_cap_usd}`);
}
run().catch(e => { console.error(e); process.exit(1); });

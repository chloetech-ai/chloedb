// Leak Lab, step 3: write leaklab_numbers.
//
//   node apps/leaklab/measure.mjs
//
// In this build every number is deterministic and needs no model: the compromised-agent
// matrix (prep.mjs runs it, load.mjs loads it), the SQL detector's agreement with the scan
// on the full bytes, the JavaScript port's agreement with the Go code, Nodwyn's own suite,
// and the corpus. This script recomputes all of them from the rows in ChloeDB and writes
// one row per displayed number (id, value, n, method, model, measured_at, cost_usd, source).
//
// The three live rounds of the spec (talk it out, the red-team agent, the follow rate) are
// NOT implemented here, and no model was called: see README.md, "Not built". numbers.mjs
// already aggregates rows of those rounds from leaklab_runs if a later build writes them.
import * as db from '../../lib/db.mjs';
import { writeNumbers } from './numbers.mjs';

const out = await writeNumbers(db, { log: console.log });
for (const r of out) console.log(`${r.id.padEnd(30)} ${String(+Number(r.value).toFixed(4)).padStart(12)}${r.k != null ? `   ${r.k}/${r.n}` : ''}`);
console.log('[measure] model spend: $0.00 (no model calls in this build)');

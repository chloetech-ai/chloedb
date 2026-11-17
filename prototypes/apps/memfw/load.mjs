// Memory Firewall: load. Creates the memfw_ tables and fills them from the artifacts under
// data/. Safe to run twice (tables are created only when missing; rows are upserted by key).
//
//   CHLOEDB_URL=http://127.0.0.1:7804 node apps/memfw/load.mjs
//
//   data/memfw.json            written by prep.mjs: the 150 poisoned documents, the 60 utility
//                              items, the templates, the probes, the base memory
//   data/recorded/*.jsonl.gz   written by measure.mjs --export: the recorded sessions, both
//                              stores, every run and the numbers, so that a deployment shows
//                              the measured results without spending on the model again
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';
import { T, DDL, baseRows } from './core.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const ARTIFACT = path.join(here, 'data', 'memfw.json');
const RECORDED = path.join(here, 'data', 'recorded');

export async function ensureTables() {
  for (const [name, ddl] of Object.entries(DDL)) {
    if (await db.ensureTable(name, ddl)) console.log(`[load] created ${name}`);
  }
}

export function readArtifact() {
  if (!fs.existsSync(ARTIFACT)) throw new Error(`${ARTIFACT} is missing: run node apps/memfw/prep.mjs first`);
  return JSON.parse(fs.readFileSync(ARTIFACT, 'utf8'));
}

async function main() {
  const h = await db.health();
  console.log(`[load] ChloeDB at ${db.BASE}: ${h.status}`);
  await ensureTables();
  const a = readArtifact();

  await db.upsertAll(T.docs, a.docs.map(d => ({
    id: d.id, n: d.n, source: d.source, source_id: d.source_id, licence: d.licence, title: d.title, attack_name: d.attack_name, category: d.category,
    template: d.template, canary: d.canary, probe: d.probe, confirm_route: d.confirm_route, placement: d.placement,
    ref_web: d.refs.web, ref_mcp: d.refs.mcp, ref_repo: d.refs.repo, payload: d.payload, wrapped: d.wrapped, text: d.text,
  })));
  await db.upsertAll(T.facts, a.facts.map(f => ({
    id: f.id, n: f.n, origin: f.origin, kind: f.kind, route: f.route || '', ref: f.ref || '', source: f.source, licence: f.licence || '',
    question: f.question, expect: f.expect, expect_re: f.expect_re || '', say: f.say || '', text: f.text || '',
  })));
  // the memory every store starts with, once per arm
  const ts = Math.floor(Date.parse(a.built_at) / 1000);
  await db.upsertAll(T.memory, [...baseRows(a.base_memory, 'off', ts), ...baseRows(a.base_memory, 'on', ts)]);
  await db.upsertAll(T.meta, [
    { id: 'templates', value: JSON.stringify(a.templates) },
    { id: 'sources', value: JSON.stringify(a.sources) },
    { id: 'probes', value: JSON.stringify(a.probes) },
    { id: 'built', value: JSON.stringify({ built_at: a.built_at, seed: a.seed }) },
  ]);
  console.log(`[load] ${a.docs.length} documents, ${a.facts.length} utility items, ${a.base_memory.length * 2} base memory rows`);

  // recorded results, when the measurement has been exported
  if (fs.existsSync(RECORDED)) {
    for (const file of fs.readdirSync(RECORDED).sort()) {
      const m = file.match(/^(memfw_[a-z0-9_]+)\.jsonl\.gz$/);
      if (!m || !DDL[m[1]]) continue;
      const rows = zlib.gunzipSync(fs.readFileSync(path.join(RECORDED, file))).toString('utf8').split('\n').filter(Boolean).map(l => JSON.parse(l));
      const have = await db.count(m[1]);
      // the measurement's own tables are filled only where they are behind the recording, so a
      // fresher local run is never overwritten by an older export
      if (m[1] !== T.memory && have >= rows.length) { console.log(`[load] ${m[1]}: ${have} rows already there, recorded ${rows.length}: kept`); continue; }
      await db.upsertAll(m[1], rows, { size: 200 });
      console.log(`[load] ${m[1]}: ${rows.length} recorded rows loaded`);
    }
  } else {
    console.log('[load] no recorded results under data/recorded yet: run node apps/memfw/measure.mjs');
  }
  for (const name of Object.keys(DDL)) console.log(`[load]   ${name}: ${await db.count(name)} rows`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();

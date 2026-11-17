// MCP Atlas — load: build the tables and fill them from the prep artifact. Idempotent;
// talks to whatever CHLOEDB_URL points at (localhost in dev, port-forwarded prod in deploy).
//
//   node apps/atlas/load.mjs            # full load (embeds 65,878 rows; several minutes)
//   ATLAS_LIMIT=4000 node apps/atlas/load.mjs   # a quick subset for dev
//   node apps/atlas/load.mjs --layout-only     # recompute only the map (islands, x, y) from
//                                              # the vectors already in the database
//   node apps/atlas/load.mjs --bindings-only   # rewrite only the bindings: Nodwyn [[bind]]
//                                              # rows, checked by Nodwyn's parser (bindcheck/,
//                                              # needs Go), and their numbers
//
// Steps: (1) create tables, (2) upsert servers (ChloeDB embeds title+description with
// potion-base-8M), (3) upsert credential bindings (an edge table) and the class nodes,
// (4) build a sensitive co-credential graph for REACHABLE, (5) read the vectors back and
// lay out topic islands (k-means + labels + packing, lib/layout.mjs) for the map, (6) write the deterministic numbers.
// Each binding also carries its row in Nodwyn's bindings.toml format (lib/nodwyn.mjs); every
// server's table is run through Nodwyn's own binding.Parse (bindcheck/) and the result stored.
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import os from 'node:os';
import { createReadStream, existsSync } from 'node:fs';
import { readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from '../../lib/db.mjs';
import { writeNumbers, writeNodwynNumbers } from './lib/numbers.mjs';
import { serverToml } from './lib/nodwyn.mjs';
import { computeIslands, K } from './lib/layout.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(HERE, 'data');
const LIMIT = Number(process.env.ATLAS_LIMIT || 0);
const LAYOUT_ONLY = process.argv.includes('--layout-only');
const BINDINGS_ONLY = process.argv.includes('--bindings-only');

const DDL = {
  atlas_servers: `CREATE TABLE atlas_servers (
    name KEYWORD PRIMARY KEY,
    title TEXT,
    description TEXT,
    publisher KEYWORD,
    status KEYWORD,
    published TIMESTAMP,
    published_day KEYWORD,
    version KEYWORD,
    reg_types KEYWORD[],
    transport KEYWORD,
    has_remote INT,
    remote_hosts KEYWORD[],
    repo_slug KEYWORD,
    website KEYWORD,
    has_secret INT,
    n_env INT,
    n_secret_env INT,
    unmarked_secretish INT,
    wants_private_key INT,
    wants_password INT,
    wants_db_url INT,
    wants_llm_key INT,
    cred_classes KEYWORD[],
    primary_class KEYWORD,
    grade KEYWORD,
    score INT,
    capability KEYWORD,
    x FLOAT,
    y FLOAT,
    island INT,
    emb VECTOR OPTIONS (embed = 'title, description', model = 'potion-base-8M')
  )`,
  atlas_bindings: `CREATE TABLE atlas_bindings (
    id KEYWORD PRIMARY KEY,
    src KEYWORD NOT NULL REFERENCES atlas_servers,
    dst KEYWORD NOT NULL,
    publisher KEYWORD,
    class KEYWORD,
    basis KEYWORD,
    dest KEYWORD,
    method KEYWORD,
    bucket KEYWORD,
    kind KEYWORD,
    rule TEXT,
    is_secret INT,
    scheme KEYWORD,
    nodwyn_row INT,
    nodwyn_class KEYWORD,
    nodwyn_dest KEYWORD,
    nodwyn_position KEYWORD,
    nodwyn_methods KEYWORD[],
    nodwyn_toml TEXT,
    nodwyn_ok INT,
    nodwyn_err TEXT
  ) OPTIONS (edges = 'src, dst')`,
  atlas_creds: `CREATE TABLE atlas_creds (
    class KEYWORD PRIMARY KEY,
    label TEXT,
    bucket KEYWORD,
    kind KEYWORD,
    dest_host KEYWORD,
    servers INT,
    example_names KEYWORD[]
  )`,
  atlas_islands: `CREATE TABLE atlas_islands (
    id INT PRIMARY KEY,
    label TEXT,
    words KEYWORD[],
    size INT,
    secret INT,
    x FLOAT,
    y FLOAT,
    r FLOAT
  )`,
  atlas_cocred: `CREATE TABLE atlas_cocred (
    id KEYWORD PRIMARY KEY,
    src KEYWORD NOT NULL REFERENCES atlas_servers,
    dst KEYWORD NOT NULL REFERENCES atlas_servers,
    class KEYWORD
  ) OPTIONS (edges = 'src, dst')`,
};

async function ensureTables() {
  for (const [name, ddl] of Object.entries(DDL)) {
    const made = await db.ensureTable(name, ddl);
    console.log(`[load] ${name}: ${made ? 'created' : 'exists'}`);
  }
}

function dayOf(published) { return (published || '').slice(0, 10); }

async function* jsonl(file) {
  const rl = createInterface({ input: createReadStream(file), crlfDelay: Infinity });
  for await (const line of rl) { if (line.trim()) yield JSON.parse(line); }
}

// The model's labels as measure.mjs recorded them (data/atlas-capability.jsonl), so that a load
// fills them in and a second load does not drop them.
async function readLabels() {
  const m = new Map();
  const file = path.join(DATA, 'atlas-capability.jsonl');
  if (!existsSync(file)) return m;
  for await (const r of jsonl(file)) m.set(r.name, r.capability);
  return m;
}
const labels = await readLabels();

async function loadServers() {
  const docs = [];
  let n = 0;
  for await (const r of jsonl(path.join(DATA, 'atlas-servers.jsonl'))) {
    docs.push({
      name: r.name, title: r.title, description: r.description, publisher: r.publisher,
      status: r.status, published: r.published || null, published_day: dayOf(r.published),
      version: r.version, reg_types: r.reg_types, transport: r.transport, has_remote: r.has_remote,
      remote_hosts: r.remote_hosts, repo_slug: r.repo_slug, website: r.website,
      has_secret: r.has_secret, n_env: r.n_env, n_secret_env: r.n_secret_env,
      unmarked_secretish: r.unmarked_secretish, wants_private_key: r.wants_private_key,
      wants_password: r.wants_password, wants_db_url: r.wants_db_url, wants_llm_key: r.wants_llm_key,
      cred_classes: r.cred_classes, primary_class: r.primary_class,
      grade: r.grade || null, score: r.score == null ? null : r.score, capability: r.capability || labels.get(r.name) || null,
      x: 0, y: 0,
    });
    if (LIMIT && ++n >= LIMIT) break;
  }
  console.log(`[load] upserting ${docs.length} servers (ChloeDB embeds each)…`);
  await db.upsertAll('atlas_servers', docs, { size: 500, onProgress: n => { if (n % 5000 === 0) console.log(`  …${n}`); } });
  return new Set(docs.map(d => d.name));
}

// Build bindcheck/ (Nodwyn's binding.go, unmodified, plus a tiny driver) into a temp dir.
// Returns the binary's path, or null (with the reason logged) when Go is missing.
function buildChecker() {
  const dir = path.join(HERE, 'bindcheck');
  const out = path.join(mkdtempSync(path.join(os.tmpdir(), 'atlasbind-')), 'atlasbind');
  try {
    execFileSync('go', ['build', '-o', out, '.'], { cwd: dir, stdio: ['ignore', 'inherit', 'inherit'] });
    return out;
  } catch (e) {
    console.error(`[load] could not build bindcheck/ (${e.code || e.message}); bindings are stored unchecked`);
    return null;
  }
}

// Run Nodwyn's parser over each server's table: items are {server, toml, want}.
function runChecker(bin, items) {
  return new Promise((resolve, reject) => {
    const p = spawn(bin, [], { stdio: ['pipe', 'pipe', 'inherit'] });
    const chunks = [];
    p.stdout.on('data', c => chunks.push(c));
    p.on('error', reject);
    p.on('close', code => {
      if (code !== 0) return reject(new Error(`bindcheck exited ${code}`));
      const lines = Buffer.concat(chunks).toString('utf8').split('\n').filter(Boolean);
      resolve(lines.map(l => JSON.parse(l)));
    });
    p.stdin.end(items.map(i => JSON.stringify(i)).join('\n') + '\n');
  });
}

async function loadBindings(keep) {
  const docs = [];
  const bySrc = new Map();
  for await (const r of jsonl(path.join(DATA, 'atlas-bindings.jsonl'))) {
    if (keep && !keep.has(r.src)) continue;
    const doc = { id: r.id, src: r.src, dst: r.dst, publisher: r.publisher, class: r.class,
      basis: r.basis, dest: r.dest, method: r.method, bucket: r.bucket, kind: r.kind,
      rule: r.rule, is_secret: r.is_secret, scheme: r.scheme || '' };
    docs.push(doc);
    if (!bySrc.has(r.src)) bySrc.set(r.src, []);
    bySrc.get(r.src).push({ ...r, doc });
  }
  // Nodwyn rows per server, then Nodwyn's own parser over each server's whole table.
  const items = [];
  const plan = new Map();
  for (const [src, rows] of bySrc) {
    const t = serverToml(src, rows);
    plan.set(src, t);
    for (const part of t.parts) {
      const d = part.b.doc;
      d.nodwyn_toml = part.toml;
      d.nodwyn_row = part.row ? 1 : 0;
      d.nodwyn_class = part.row ? part.row.class : '';
      d.nodwyn_dest = part.row ? part.row.dest : '';
      d.nodwyn_position = part.row ? part.row.positions[0] : '';
      d.nodwyn_methods = part.row ? part.row.methods : [];
      d.nodwyn_ok = part.row ? -1 : null;      // -1: not checked (yet)
      d.nodwyn_err = part.row ? '' : part.why;
    }
    if (t.want.length) items.push({ server: src, toml: t.toml, want: t.want });
  }
  const bin = buildChecker();
  if (bin) {
    const t0 = Date.now();
    const results = await runChecker(bin, items);
    let accepted = 0, rows = 0;
    for (const res of results) {
      const t = plan.get(res.server);
      const withRow = t.parts.filter(p => p.row);
      withRow.forEach((p, i) => {
        rows++;
        const ok = res.ok && res.row_ok[i] === true;
        if (ok) accepted++;
        p.b.doc.nodwyn_ok = ok ? 1 : 0;
        p.b.doc.nodwyn_err = res.ok ? (ok ? '' : 'Parse accepted the table but returned a different row') : res.err;
      });
    }
    console.log(`[load] Nodwyn's parser: ${accepted} of ${rows} rows accepted, over ${results.length} servers' tables (${Date.now() - t0} ms)`);
  }
  console.log(`[load] upserting ${docs.length} bindings…`);
  await db.upsertAll('atlas_bindings', docs, { size: 1000 });
  return { checked: !!bin };
}

// Tables made by an earlier version lack the Nodwyn columns; add them in place.
async function ensureBindingColumns() {
  const sc = await db.schema('atlas_bindings');
  const have = new Set(((sc && sc.schema && sc.schema.fields) || []).map(f => f.name));
  const want = [['scheme', 'KEYWORD'], ['nodwyn_row', 'INT'], ['nodwyn_class', 'KEYWORD'], ['nodwyn_dest', 'KEYWORD'],
    ['nodwyn_position', 'KEYWORD'], ['nodwyn_methods', 'KEYWORD[]'], ['nodwyn_toml', 'TEXT'], ['nodwyn_ok', 'INT'], ['nodwyn_err', 'TEXT']];
  for (const [name, type] of want) {
    if (have.has(name)) continue;
    await db.sql(`ALTER TABLE atlas_bindings ADD COLUMN ${name} ${type}`);
    console.log(`[load] atlas_bindings: added column ${name}`);
  }
}

async function loadCreds(keep) {
  let creds = JSON.parse(readFileSync(path.join(DATA, 'atlas-creds.json'), 'utf8'));
  // when a subset is loaded, recount from the kept bindings is skipped — counts are full-registry
  const docs = creds.map(c => ({ class: c.class, label: c.label, bucket: c.bucket, kind: c.kind,
    dest_host: c.dest_host, servers: c.servers, example_names: c.example_names }));
  console.log(`[load] upserting ${docs.length} credential classes…`);
  await db.upsertAll('atlas_creds', docs, { size: 500 });
}

// A connectivity chain per sensitive class so REACHABLE returns the whole shared-credential
// cluster. src/dst both reference atlas_servers, so the walk is genuinely server->server.
async function buildCoCred() {
  const classes = ['private-key', 'password', 'database'];
  const edges = [];
  for (const cls of classes) {
    const r = await db.query(
      `SELECT DISTINCT src FROM atlas_bindings WHERE bucket = :b ORDER BY src LIMIT 10000`, { b: cls });
    const servers = r.rows.map(x => x.src);
    for (let i = 0; i + 1 < servers.length; i++) {
      const a = servers[i], b = servers[i + 1];
      edges.push({ id: `${cls}|${a}>${b}`, src: a, dst: b, class: cls });
      edges.push({ id: `${cls}|${b}>${a}`, src: b, dst: a, class: cls });
    }
  }
  console.log(`[load] co-credential edges: ${edges.length}`);
  if (edges.length) await db.upsertAll('atlas_cocred', edges, { size: 1000 });
}

// ---- topic-island layout (lib/layout.mjs) ---------------------------------------------
async function readEmbeddings(names) {
  const dim = 256;
  const buf = new Float32Array(names.length * dim);
  const texts = new Array(names.length).fill('');
  const pubs = new Array(names.length).fill('');
  const idx = new Map(names.map((n, i) => [n, i]));
  let got = 0;
  const size = 2000;
  for (let i = 0; i < names.length; i += size) {
    const batch = names.slice(i, i + size);
    const r = await db.query(
      `SELECT name, title, description, publisher, emb FROM atlas_servers WHERE name IN (${batch.map((_, k) => `:n${k}`).join(',')})`,
      Object.fromEntries(batch.map((n, k) => [`n${k}`, n])));
    for (const row of r.rows) {
      const j = idx.get(row.name); if (j == null || !row.emb) continue;
      const e = row.emb; for (let d = 0; d < dim; d++) buf[j * dim + d] = e[d];
      texts[j] = (row.title || row.name.split('/').pop()) + ' ' + (row.description || '');
      pubs[j] = row.publisher || '';
      got++;
    }
    if (i % 20000 === 0) console.log(`  …read ${i}`);
  }
  console.log(`[load] read ${got}/${names.length} embeddings`);
  return { buf, dim, texts, pubs };
}

// A write after a long computation can meet a keep-alive socket the server already closed;
// retry such transient failures (the writes are idempotent upserts).
async function retry(fn, tries = 4) {
  for (let t = 1; ; t++) {
    try { return await fn(); }
    catch (e) { if (t >= tries || !/fetch failed|socket|ECONNRESET|timed out/i.test(String(e.message) + String(e.cause?.code || ''))) throw e; await new Promise(r => setTimeout(r, 500 * t)); }
  }
}

async function writeLayout(names, coords, assign) {
  const size = 800;
  for (let i = 0; i < names.length; i += size) {
    const batch = names.slice(i, i + size);
    const vals = batch.map((n, k) => `(:n${k}, :x${k}, :y${k}, :i${k})`).join(',');
    const params = {};
    batch.forEach((n, k) => { params[`n${k}`] = n; params[`x${k}`] = +coords[i + k][0].toFixed(2); params[`y${k}`] = +coords[i + k][1].toFixed(2); params[`i${k}`] = assign[i + k]; });
    await retry(() => db.sql(`INSERT INTO atlas_servers (name, x, y, island) VALUES ${vals} ON CONFLICT (name) DO UPDATE SET x = excluded.x, y = excluded.y, island = excluded.island`, params));
  }
  console.log(`[load] wrote layout for ${names.length} servers`);
}

// The islands table: id, label, words, size, how many declare a secret (counted by SQL after
// the servers' island ids are written), centre and radius on the map.
async function writeIslands(islands) {
  const sec = await db.query(`SELECT island, COUNT(*) AS n FROM atlas_servers WHERE has_secret = 1 GROUP BY island LIMIT 1000`);
  const secBy = new Map(sec.rows.map(r => [Number(r.island), Number(r.n)]));
  const cnt = await db.query(`SELECT island, COUNT(*) AS n FROM atlas_servers GROUP BY island LIMIT 1000`);
  const sizeBy = new Map(cnt.rows.map(r => [Number(r.island), Number(r.n)]));
  await db.sql(`DELETE FROM atlas_islands WHERE id >= 0`);
  const docs = islands.map(t => ({ id: t.id, label: t.label, words: t.words, size: sizeBy.get(t.id) ?? t.size,
    secret: secBy.get(t.id) || 0, x: t.x, y: t.y, r: t.r }));
  await db.upsertAll('atlas_islands', docs, { size: 200 });
  console.log(`[load] wrote ${docs.length} islands`);
  for (const d of docs.slice().sort((a, b) => b.size - a.size).slice(0, 12)) console.log(`   ${String(d.size).padStart(6)}  ${String(d.secret).padStart(5)} secret  ${d.label}`);
}

// Near-duplicate share: within a publisher, two servers whose embeddings have cosine
// similarity above 0.95. Counts servers with at least one near-twin. Uses the vectors we
// already read for the layout; per-publisher work is capped so the big publishers are sampled.
function nearDupCount(buf, dim, names) {
  const n = names.length;
  const norm = new Float32Array(n);
  for (let i = 0; i < n; i++) { let s = 0; for (let d = 0; d < dim; d++) { const v = buf[i * dim + d]; s += v * v; } norm[i] = Math.sqrt(s) || 1; }
  const groups = new Map();
  for (let i = 0; i < n; i++) { const p = names[i].includes('/') ? names[i].split('/')[0] : names[i]; (groups.get(p) || groups.set(p, []).get(p)).push(i); }
  const CAP = 1500; // compare at most CAP members per publisher
  const flagged = new Set();
  for (const idxs of groups.values()) {
    if (idxs.length < 2) continue;
    const m = idxs.length <= CAP ? idxs : idxs.filter((_, k) => k % Math.ceil(idxs.length / CAP) === 0);
    for (let a = 0; a < m.length; a++) {
      const ia = m[a]; if (flagged.has(ia)) { /* still may flag partner */ }
      for (let b = a + 1; b < m.length; b++) {
        const ib = m[b];
        let dot = 0; for (let d = 0; d < dim; d++) dot += buf[ia * dim + d] * buf[ib * dim + d];
        if (dot / (norm[ia] * norm[ib]) > 0.95) { flagged.add(ia); flagged.add(ib); }
      }
    }
  }
  return flagged.size;
}

// Read every row's key, a page at a time (the server returns at most 10,000 per query).
async function allNames() {
  const names = [];
  const page = 10000;
  for (let off = 0; ; off += page) {
    const r = await db.query(`SELECT name FROM atlas_servers ORDER BY name LIMIT ${page} OFFSET ${off}`);
    if (!r.rows.length) break;
    for (const row of r.rows) names.push(row.name);
    if (r.rows.length < page) break;
  }
  return names;
}

async function layout({ nearDup = true } = {}) {
  const names = await allNames();
  const { buf, dim, texts, pubs } = await readEmbeddings(names);
  console.log(`[load] topic islands: k-means (k=${K}) on ${names.length} vectors…`);
  const t0 = Date.now();
  const { coords, assign, islands } = computeIslands(buf, dim, names, texts, { pubs });
  console.log(`[load] islands computed in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
  await writeLayout(names, coords, assign);
  await writeIslands(islands);
  if (!nearDup) return null;
  console.log('[load] counting near-duplicates…');
  const nd = nearDupCount(buf, dim, names);
  console.log(`[load] near-duplicate servers (cosine>0.95 within publisher): ${nd}`);
  return nd;
}

// Tables made by an earlier version lack the island column; add it in place.
async function ensureIslandColumn() {
  const sc = await db.schema('atlas_servers');
  const fields = (sc && sc.schema && sc.schema.fields) || [];
  if (!fields.some(f => f.name === 'island')) {
    await db.sql('ALTER TABLE atlas_servers ADD COLUMN island INT');
    console.log('[load] atlas_servers: added column island');
  }
}

async function main() {
  const t0 = Date.now();
  await db.health();
  await ensureTables();
  await ensureIslandColumn();
  await ensureBindingColumns();
  if (BINDINGS_ONLY) {
    // Rewrite only the bindings (Nodwyn rows and their parse) for the servers already loaded,
    // and the Nodwyn numbers; no server, layout or other number changes.
    const keep = new Set(await allNames());
    const { checked } = await loadBindings(keep);
    const n = await writeNodwynNumbers(db, { checked });
    console.log(`[load] wrote ${n} Nodwyn number rows`);
    console.log(`[load] bindings-only done in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
    return;
  }
  if (LAYOUT_ONLY) {
    // Recompute only the map from the vectors already in the database; no rows, bindings
    // or numbers change.
    await layout({ nearDup: false });
    console.log(`[load] layout-only done in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
    return;
  }
  const keep = await loadServers();
  const { checked } = await loadBindings(keep);
  await loadCreds(keep);
  await buildCoCred();
  const nd = await layout();
  const total = (await db.query('SELECT COUNT(*) AS v FROM atlas_servers')).rows[0].v;
  const extra = [{ id: 'near_dup', value: nd, n: total, pct: total ? +(nd / total).toFixed(4) : null,
    label: 'Near-duplicate servers (cosine > 0.95 within a publisher)', method: 'vector scan at load (potion-base-8M embeddings)',
    source: 'mcp-registry', note: 'the big publishers are sampled; see README', sql_text: 'cosine(emb_i, emb_j) > 0.95 for i, j in the same publisher — computed in Node from the embeddings ChloeDB built' }];
  const nums = await writeNumbers(db, { withModel: labels.size > 0, extra }) + await writeNodwynNumbers(db, { checked });
  console.log(`[load] wrote ${nums} number rows${labels.size ? ` (with the ${labels.size} recorded model labels)` : ''}`);
  console.log(`[load] done in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
}
main().catch(e => { console.error(e); process.exit(1); });

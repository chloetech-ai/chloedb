// MCP Atlas — the API. Every view's SQL rides back in `queries` for the drawer.
import { BUCKETS, BUCKET_PRIORITY } from './lib/creds.mjs';
import { joinToml, NODWYN_PARSER } from './lib/nodwyn.mjs';

export const meta = {
  title: 'MCP Atlas',
  tagline: 'Every MCP server, mapped by the credential it asks for — and the least-privilege binding that would contain it.',
  order: 5,
  // The hub card: rows of atlas_numbers (value, and n where it is a share).
  hub: [
    { id: 'servers', label: 'MCP servers mapped, by the credential each declares', format: 'n' },
    { id: 'secret_servers', label: 'declare a secret', format: 'kn' },
    { id: 'unmarked', label: 'variables named like a secret but not marked secret', format: 'n' },
    { id: 'nodwyn_accepted', label: "generated Nodwyn bindings that Nodwyn's own parser accepts", format: 'kn' },
  ],
};

// Coarse buckets -> a map colour (distinct on the dark field).
const BUCKET_COLOR = {
  none: '#46507a', 'api-key': '#5b6cff', llm: '#a78bfa', vcs: '#22d3ee', cloud: '#34d399',
  saas: '#60a5fa', send: '#fbbf24', pay: '#2dd4bf', database: '#fb923c', password: '#f472b6', 'private-key': '#ff5470',
};
const BUCKET_LIST = BUCKET_PRIORITY.slice().reverse().map(k => ({ key: k, label: BUCKETS[k].label, color: BUCKET_COLOR[k] }));

const q = (r, params) => ({ sql: r.sql, params: params || {}, took_ms: r.took_ms, rows: r.rows.length, plan: r.plan });

let pointsCache = null;

async function buildPoints(db) {
  const bucketIdx = Object.fromEntries(BUCKET_PRIORITY.map((k, i) => [k, i]));
  const gradeIdx = { A: 1, B: 2, C: 3, D: 4, F: 5 };
  const rows = [];
  const queries = [];
  const batch = 10000; // the server returns at most 10,000 rows per query
  // published range for the time slider
  const rng = await db.query(`SELECT MIN(published_day) AS lo, MAX(published_day) AS hi FROM atlas_servers WHERE published_day <> ''`);
  queries.push(q(rng));
  const lo = rng.rows[0].lo, hi = rng.rows[0].hi;
  const dayNum = s => { if (!s) return 0; const d = new Date(s + 'T00:00:00Z'), b = new Date((lo || s) + 'T00:00:00Z'); return Math.max(0, Math.round((d - b) / 86400000)); };
  for (let off = 0; ; off += batch) {
    const r = await db.query(
      `SELECT name, x, y, island, primary_class AS cls, published_day AS day, grade, has_secret AS sec FROM atlas_servers ORDER BY name LIMIT ${batch} OFFSET ${off}`);
    if (off === 0) queries.push(q(r));
    if (!r.rows.length) break;
    for (const row of r.rows) rows.push(row);
    if (r.rows.length < batch) break;
  }
  // the topic islands: label, size, how many declare a secret, centre and radius
  let islands = [];
  try {
    const isl = await db.query(`SELECT id, label, size, secret, x, y, r FROM atlas_islands ORDER BY size DESC LIMIT 500`);
    queries.push(q(isl)); islands = isl.rows;
  } catch { /* table not there yet: the map draws without labels */ }
  const n = rows.length;
  const I = new Array(n);
  const X = new Array(n), Y = new Array(n), C = new Array(n), D = new Array(n), G = new Array(n), S = new Array(n), N = new Array(n);
  for (let i = 0; i < n; i++) {
    const r = rows[i];
    X[i] = +(r.x || 0).toFixed(1); Y[i] = +(r.y || 0).toFixed(1); I[i] = r.island == null ? -1 : r.island;
    C[i] = bucketIdx[r.cls] ?? bucketIdx.none;
    D[i] = dayNum(r.day); G[i] = gradeIdx[r.grade] || 0; S[i] = r.sec ? 1 : 0; N[i] = r.name;
  }
  return { n, x: X, y: Y, island: I, islands, cls: C, day: D, grade: G, secret: S, names: N,
    buckets: BUCKET_LIST, bucketOrder: BUCKET_PRIORITY, colors: BUCKET_COLOR,
    day_lo: lo, day_hi: hi, day_span: dayNum(hi), built_at: Date.now(), queries };
}

export async function init(ctx) {
  // warm the points cache so the map loads fast; tolerate an empty DB
  try { pointsCache = await buildPoints(ctx.db); console.log(`[atlas] points cached: ${pointsCache.n}`); }
  catch (e) { console.error('[atlas] points cache failed (load not run yet?):', e.message); }
}

export async function handle(ctx) {
  const { db, json, res, route, query } = ctx;

  if (route === '/numbers') {
    const r = await db.query(`SELECT id, value, n, pct, label, method, model, source, note, sql_text, measured_at FROM atlas_numbers LIMIT 500`);
    const by = {}; for (const row of r.rows) by[row.id] = row;
    return json(res, 200, { numbers: by, list: r.rows, queries: [q(r)] });
  }

  if (route === '/points') {
    // rebuilt when older than five minutes, so a `load.mjs --layout-only` run shows without a restart
    if (!pointsCache || Date.now() - pointsCache.built_at > 300_000) {
      try { pointsCache = await buildPoints(db); } catch (e) { if (!pointsCache) return json(res, 503, { error: 'map not loaded yet: ' + e.message }); }
    }
    return json(res, 200, pointsCache);
  }

  if (route === '/creds') {
    const r = await db.query(`SELECT class, label, bucket, kind, dest_host, servers, example_names FROM atlas_creds ORDER BY servers DESC LIMIT 200`);
    return json(res, 200, { creds: r.rows, colors: BUCKET_COLOR, queries: [q(r)] });
  }

  if (route === '/server') {
    const name = query.get('name'); if (!name) return json(res, 400, { error: 'name required' });
    const queries = [];
    const sr = await db.query(`SELECT * FROM atlas_servers WHERE name = :n LIMIT 1`, { n: name });
    queries.push(q(sr, { n: name }));
    const s = sr.rows[0]; if (!s) return json(res, 404, { error: 'no such server' });
    delete s.emb;
    // each binding's row in Nodwyn's bindings.toml format, and what Nodwyn's parser said (load.mjs)
    const br = await db.query(`SELECT id, class, basis, dest, method, bucket, kind, rule, is_secret, nodwyn_row, nodwyn_class, nodwyn_dest, nodwyn_position, nodwyn_toml, nodwyn_ok, nodwyn_err FROM atlas_bindings WHERE src = :n ORDER BY id LIMIT 100`, { n: name });
    queries.push(q(br, { n: name }));
    const bindings = br.rows.map(b => ({ ...b, toml: b.nodwyn_toml || '' }));
    const withRow = bindings.filter(b => b.nodwyn_row === 1);
    const nodwyn = {
      parser: NODWYN_PARSER,
      rows: withRow.length,
      accepted: withRow.filter(b => b.nodwyn_ok === 1).length,
      rejected: withRow.filter(b => b.nodwyn_ok === 0).length,
      unchecked: withRow.filter(b => b.nodwyn_ok !== 0 && b.nodwyn_ok !== 1).length,
      error: (withRow.find(b => b.nodwyn_ok === 0) || {}).nodwyn_err || '',
      file: bindings.length ? joinToml(name, bindings.map(b => b.toml)) : '',
    };
    // similar servers (hybrid rrf), excluding same name
    const simParams = { q: (s.title + ' ' + s.description).slice(0, 300), n: name };
    const sim = await db.query(
      `SELECT name, title, publisher, primary_class FROM atlas_servers WHERE name <> :n ORDER BY rrf(bm25(description, :q), similarity(emb, :q)) DESC MAX 1 PER publisher LIMIT 8`, simParams);
    queries.push(q(sim, simParams));
    // publisher's other servers
    const pub = await db.query(`SELECT name, title, has_secret, primary_class FROM atlas_servers WHERE publisher = :p AND name <> :n LIMIT 8`, { p: s.publisher, n: name });
    queries.push(q(pub, { p: s.publisher }));
    return json(res, 200, { server: s, bindings, nodwyn, similar: sim.rows, publisher_others: pub.rows, queries });
  }

  if (route === '/cred') {
    const cls = query.get('class'); if (!cls) return json(res, 400, { error: 'class required' });
    const queries = [];
    const info = await db.query(`SELECT class, label, bucket, kind, dest_host, servers, example_names FROM atlas_creds WHERE class = :c LIMIT 1`, { c: cls });
    queries.push(q(info, { c: cls }));
    const servers = await db.query(
      `SELECT b.src AS name, b.basis AS basis, b.dest AS dest, b.method AS method, s.title AS title, s.publisher AS publisher, s.grade AS grade, s.primary_class AS primary_class FROM atlas_bindings b JOIN atlas_servers s ON b.src = s.name WHERE b.class = :c LIMIT 400`, { c: cls });
    queries.push(q(servers, { c: cls }));
    // what one leaked token reaches: the distinct destinations across those servers' same-class bindings
    const reach = await db.query(`SELECT DISTINCT dest FROM atlas_bindings WHERE class = :c AND dest <> '' LIMIT 50`, { c: cls });
    queries.push(q(reach, { c: cls }));
    return json(res, 200, { info: info.rows[0] || { class: cls }, servers: servers.rows, reaches: reach.rows.map(r => r.dest), queries });
  }

  if (route === '/search') {
    const raw = (query.get('q') || '').trim();
    const credFilter = query.get('cred') || '';
    const capFilter = query.get('cap') || '';
    const queries = [];
    // A filtered hybrid search. Filters use a subquery over the bindings edge table.
    let usedParams = { q: raw || 'mcp server' };
    const conds = [];
    if (credFilter) { conds.push(`name IN (SELECT src FROM atlas_bindings WHERE class = :cred)`); usedParams.cred = credFilter; }
    if (capFilter) { conds.push(`capability = :cap`); usedParams.cap = capFilter; }
    const whereSql = conds.length ? ` WHERE ${conds.join(' AND ')}` : '';
    const sql = `SELECT name, title, publisher, primary_class, grade, has_secret FROM atlas_servers${whereSql} ORDER BY rrf(bm25(description, :q), similarity(emb, :q)) DESC MAX 2 PER publisher LIMIT 24`;
    const r = await db.query(sql, usedParams);
    queries.push(q(r, usedParams));
    return json(res, 200, { results: r.rows, queries });
  }

  if (route === '/reachable') {
    const name = query.get('name'); if (!name) return json(res, 400, { error: 'name required' });
    const params = { s: name };
    const r = await db.query(
      `SELECT name, publisher, title, grade, primary_class FROM atlas_servers WHERE REACHABLE(atlas_cocred, :s, dir => 'both', hops => 9999) AND name <> :s LIMIT 400`, params);
    return json(res, 200, { cluster: r.rows, queries: [q(r, params)] });
  }

  if (route === '/classify') {
    const name = query.get('name'); if (!name) return json(res, 400, { error: 'name required' });
    const sr = await db.query(`SELECT name, title, description, capability FROM atlas_servers WHERE name = :n LIMIT 1`, { n: name });
    const s = sr.rows[0]; if (!s) return json(res, 404, { error: 'no such server' });
    if (s.capability) return json(res, 200, { capability: s.capability, live: false, recorded: true, queries: [q(sr, { n: name })] });
    try {
      const resp = await ctx.llm.complete({
        app: 'atlas', ip: ctx.ip, model: ctx.llm.CHEAP_MODEL, max_tokens: 20, effort: 'low',
        system: 'You label what an MCP server can do with the credential it asks for. Answer with exactly one word from: read, write, execute, send, pay, unclear.',
        messages: [{ role: 'user', content: `Title: ${s.title}\nDescription: ${s.description}\n\nOne word:` }],
      });
      let cap = (ctx.llm.textOf(resp) || '').toLowerCase().trim().replace(/[^a-z]/g, '');
      if (!['read', 'write', 'execute', 'send', 'pay', 'unclear'].includes(cap)) cap = 'unclear';
      await db.sql(`INSERT INTO atlas_servers (name, capability) VALUES (:n, :c) ON CONFLICT (name) DO UPDATE SET capability = excluded.capability`, { n: name, c: cap });
      return json(res, 200, { capability: cap, live: true, cost_usd: resp.cost_usd, queries: [q(sr, { n: name })] });
    } catch (e) {
      return json(res, 200, { capability: null, live: false, error: e.message, code: e.code, queries: [q(sr, { n: name })] });
    }
  }

  return false;
}

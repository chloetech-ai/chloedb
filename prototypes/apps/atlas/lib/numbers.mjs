// MCP Atlas — the numbers table. Every figure the page shows is one row here, computed by
// a SQL COUNT/SUM over the loaded tables (or, for the two that need vectors/model, measured
// and labelled as such). The page reads /atlas/api/numbers and renders only rows it finds.
//
// Row: id, value, n (denominator for a share), pct, label, method, model, source, note,
//      sql_text (shown in the SQL drawer), measured_at, cost_usd.

export const NUMBERS_DDL = `CREATE TABLE atlas_numbers (
  id KEYWORD PRIMARY KEY,
  value FLOAT,
  n INT,
  pct FLOAT,
  label TEXT,
  method KEYWORD,
  model KEYWORD,
  source KEYWORD,
  note TEXT,
  sql_text TEXT,
  measured_at TIMESTAMP,
  cost_usd FLOAT
)`;

// Each deterministic number: [id, label, sqlText (returns one value in column v), denom].
// denom is 'servers' | 'bindings' | null (the share's denominator, fetched once).
const DETERMINISTIC = [
  ['servers', 'Servers in the registry', `SELECT COUNT(*) AS v FROM atlas_servers`, null],
  ['publishers', 'Distinct publishers', `SELECT COUNT(DISTINCT publisher) AS v FROM atlas_servers`, null],
  ['secret_servers', 'Servers that declare a secret', `SELECT COUNT(*) AS v FROM atlas_servers WHERE has_secret = 1`, 'servers'],
  ['secret_env', 'Secret environment variables declared', `SELECT SUM(n_secret_env) AS v FROM atlas_servers`, null],
  ['unmarked', 'Env vars named like key/token/secret/password NOT marked secret', `SELECT SUM(unmarked_secretish) AS v FROM atlas_servers`, null],
  ['private_key', 'Servers asking for a private key, seed phrase or wallet', `SELECT COUNT(*) AS v FROM atlas_servers WHERE wants_private_key = 1`, 'servers'],
  ['password', 'Servers asking for a password', `SELECT COUNT(*) AS v FROM atlas_servers WHERE wants_password = 1`, 'servers'],
  ['db_url', 'Servers asking for a database URL', `SELECT COUNT(*) AS v FROM atlas_servers WHERE wants_db_url = 1`, 'servers'],
  ['llm_key', 'Servers asking for an LLM provider key', `SELECT COUNT(*) AS v FROM atlas_servers WHERE wants_llm_key = 1`, 'servers'],
  ['since_oct', 'Servers published since 1 October 2026', `SELECT COUNT(*) AS v FROM atlas_servers WHERE published >= '2026-10-01'`, 'servers'],
  ['secret_header_servers', 'Remote endpoints with a declared secret header', `SELECT COUNT(DISTINCT src) AS v FROM atlas_bindings WHERE method LIKE 'header:%'`, 'servers'],
  ['bindings', 'Credential bindings generated', `SELECT COUNT(*) AS v FROM atlas_bindings`, null],
  ['basis_exact', 'Bindings with an exact destination (remote URL + secret header)', `SELECT COUNT(*) AS v FROM atlas_bindings WHERE basis = 'exact'`, 'bindings'],
  ['basis_inferred', 'Bindings inferred from a known variable-name pattern', `SELECT COUNT(*) AS v FROM atlas_bindings WHERE basis = 'inferred'`, 'bindings'],
  ['basis_unknown', 'Bindings with no known destination (unbound: denied by default)', `SELECT COUNT(*) AS v FROM atlas_bindings WHERE basis = 'unknown'`, 'bindings'],
  ['bound_hosts', 'Bindings that bind to a named host (exact + inferred)', `SELECT COUNT(*) AS v FROM atlas_bindings WHERE dest <> ''`, 'bindings'],
  ['graded_repos', 'Repositories that join MCPShield (of 65,878)', `SELECT COUNT(DISTINCT repo_slug) AS v FROM atlas_servers WHERE grade IS NOT NULL AND repo_slug <> ''`, 'servers'],
];

const top5SQL = `SELECT SUM(c) AS v FROM (SELECT publisher, COUNT(*) AS c FROM atlas_servers GROUP BY publisher ORDER BY c DESC LIMIT 5) AS t`;

async function one(db, sqlText) {
  const r = await db.query(sqlText);
  const row = r.rows[0] || {};
  return { v: Number(row.v || 0), took_ms: r.took_ms, plan: r.plan };
}

export async function writeNumbers(db, { withModel = false, extra = [] } = {}) {
  await db.ensureTable('atlas_numbers', NUMBERS_DDL);
  const now = new Date().toISOString();
  const rows = [];
  const denoms = {
    servers: (await one(db, `SELECT COUNT(*) AS v FROM atlas_servers`)).v,
    bindings: (await one(db, `SELECT COUNT(*) AS v FROM atlas_bindings`)).v,
  };

  for (const [id, label, sqlText, denom] of DETERMINISTIC) {
    const { v } = await one(db, sqlText);
    const d = denom ? denoms[denom] : null;
    rows.push({ id, value: v, n: d, pct: d ? +(v / d).toFixed(4) : null, label,
      method: 'SQL count at load', model: null, source: 'mcp-registry', note: '', sql_text: sqlText, measured_at: now, cost_usd: 0 });
  }
  // top-5 publisher share
  { const { v } = await one(db, top5SQL); const d = denoms.servers;
    rows.push({ id: 'top5_share', value: v, n: d, pct: d ? +(v / d).toFixed(4) : null,
      label: 'Share of the registry from the five largest publishers', method: 'SQL count at load',
      model: null, source: 'mcp-registry', note: '', sql_text: top5SQL, measured_at: now, cost_usd: 0 }); }

  // grade distribution over distinct repos
  { const r = await db.query(`SELECT grade AS g, COUNT(DISTINCT repo_slug) AS c FROM atlas_servers WHERE grade IS NOT NULL AND repo_slug <> '' GROUP BY grade ORDER BY grade`);
    for (const row of r.rows) {
      rows.push({ id: `grade_${row.g}`, value: Number(row.c), n: null, pct: null,
        label: `MCPShield grade ${row.g} (distinct repositories)`, method: 'SQL count at load, grade from MCPShield',
        model: null, source: 'mcpshield-scan-2026', note: "MCPShield's grade, for the repos that join", sql_text: r.sql, measured_at: now, cost_usd: 0 }); }
  }

  for (const e of extra) rows.push({ measured_at: now, cost_usd: 0, model: null, ...e });

  if (withModel) {
    // capability classification coverage (filled by measure.mjs; counted here if present)
    const secret = (await one(db, `SELECT COUNT(*) AS v FROM atlas_servers WHERE has_secret = 1`)).v;
    const caps = ['read', 'write', 'execute', 'send', 'pay', 'unclear'];
    for (const c of caps) {
      const sqlText = `SELECT COUNT(*) AS v FROM atlas_servers WHERE capability = '${c}'`;
      const { v } = await one(db, sqlText);
      rows.push({ id: `cap_${c}`, value: v, n: secret, pct: secret ? +(v / secret).toFixed(4) : null,
        label: `Secret-bearing servers classified "${c}"`, method: 'from the description, by a model',
        model: 'claude-haiku-5-5', source: 'model', note: 'capability the description implies for the credential', sql_text: sqlText, measured_at: now, cost_usd: 0 });
    }
    const classified = (await one(db, `SELECT COUNT(*) AS v FROM atlas_servers WHERE has_secret = 1 AND capability IS NOT NULL AND capability <> ''`)).v;
    rows.push({ id: 'cap_classified', value: classified, n: secret, pct: secret ? +(classified / secret).toFixed(4) : null,
      label: 'Secret-bearing servers classified by capability', method: 'from the description, by a model',
      model: 'claude-haiku-5-5', source: 'model', note: '', sql_text: `SELECT COUNT(*) FROM atlas_servers WHERE has_secret = 1 AND capability <> ''`, measured_at: now, cost_usd: 0 });
  }

  await db.upsertAll('atlas_numbers', rows, { size: 100 });
  return rows.length;
}

// The Nodwyn rows (lib/nodwyn.mjs) and what Nodwyn's own parser said of them (bindcheck/, run
// at load). checked=false (no Go toolchain) writes the generated counts but not "accepted".
export async function writeNodwynNumbers(db, { checked = true } = {}) {
  await db.ensureTable('atlas_numbers', NUMBERS_DDL);
  const now = new Date().toISOString();
  const bound = (await one(db, `SELECT COUNT(*) AS v FROM atlas_bindings WHERE basis <> 'unknown'`)).v;
  const PARSED = new Set(['nodwyn_accepted', 'nodwyn_rejected', 'nodwyn_servers_accepted']);
  const methodOf = id => PARSED.has(id) ? 'Nodwyn binding.Parse at load (Go, cdc30069)' : 'SQL count at load';
  const defs = [
    ['nodwyn_rows', 'Nodwyn [[bind]] rows generated (exact + inferred bindings with a host)',
      `SELECT COUNT(*) AS v FROM atlas_bindings WHERE nodwyn_row = 1`, bound],
    ['nodwyn_servers', 'Servers with at least one Nodwyn row',
      `SELECT COUNT(DISTINCT src) AS v FROM atlas_bindings WHERE nodwyn_row = 1`, null],
    ['nodwyn_no_host', 'Exact or inferred bindings with no row: no host name to bind (a variable in the URL, or a database URL\'s value)',
      `SELECT COUNT(*) AS v FROM atlas_bindings WHERE basis <> 'unknown' AND nodwyn_row = 0`, bound],
    ['nodwyn_wildcard', 'Rows that bind subdomains (*.domain): a provider\'s account hosts, or a URL whose subdomain is a variable',
      `SELECT COUNT(*) AS v FROM atlas_bindings WHERE nodwyn_row = 1 AND nodwyn_dest LIKE '*.%'`, null],
    ['nodwyn_bearer_assumed', 'Header rows whose Bearer scheme is Nodwyn\'s default (the record names none)',
      `SELECT COUNT(*) AS v FROM atlas_bindings WHERE nodwyn_row = 1 AND kind = 'header' AND scheme = '' AND nodwyn_position LIKE 'header:%:Bearer'`, null],
  ];
  if (checked) defs.push(
    ['nodwyn_accepted', 'Nodwyn rows accepted by Nodwyn\'s parser (and decoded back unchanged)',
      `SELECT COUNT(*) AS v FROM atlas_bindings WHERE nodwyn_ok = 1`, 'rows'],
    ['nodwyn_rejected', 'Nodwyn rows its parser rejected',
      `SELECT COUNT(*) AS v FROM atlas_bindings WHERE nodwyn_ok = 0`, 'rows'],
    ['nodwyn_servers_accepted', 'Servers whose whole bindings.toml Nodwyn\'s parser accepts',
      `SELECT COUNT(DISTINCT src) AS v FROM atlas_bindings WHERE nodwyn_ok = 1`, 'servers'],
  );
  const rows = [];
  const got = {};
  for (const [id, label, sqlText, denom] of defs) {
    const { v } = await one(db, sqlText);
    got[id] = v;
    const d = denom === 'rows' ? got.nodwyn_rows : denom === 'servers' ? got.nodwyn_servers : denom;
    rows.push({ id, value: v, n: d, pct: d ? +(v / d).toFixed(4) : null, label, method: methodOf(id), model: null,
      source: 'mcp-registry', note: 'rows in Nodwyn\'s bindings.toml format; no row for an unknown destination (Nodwyn denies unbound egress by default, ADR 6)',
      sql_text: sqlText, measured_at: now, cost_usd: 0 });
  }
  await db.upsertAll('atlas_numbers', rows, { size: 100 });
  return rows.length;
}

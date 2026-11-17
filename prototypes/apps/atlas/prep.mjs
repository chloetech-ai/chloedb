// MCP Atlas — prep: read the datasets HERE and write a compact artifact for load.mjs.
//
// Reads (local only; the deploy container has none of these):
//   /home/user/datasets/mcp-registry/servers-latest.jsonl   65,878 servers (authoritative)
//   /home/user/datasets/mcpshield-scan-2026/...jsonl         MCPShield grades (CC-BY-4.0)
//
// Writes under apps/atlas/data/:
//   atlas-servers.jsonl   one row per server (no emb/x/y — ChloeDB computes those at load)
//   atlas-bindings.jsonl  one row per declared credential binding (exact/inferred/unknown)
//   atlas-creds.json      the credential-class node table with server counts
//   atlas-stats.json      sanity counts (not displayed; the page reads SQL rows)
//
// The servers and bindings artifacts (47 MB and 7 MB) are not committed: this rebuilds them,
// and load.mjs loads them into a database from a machine that has them.
import { readFileSync, writeFileSync, createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { classifyEnvVar, classifyHeader, pickPrimaryBucket, KEYISH_RE, BUCKETS } from './lib/creds.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DATA = path.join(HERE, 'data');
const REG = process.env.ATLAS_REGISTRY || '/home/user/datasets/mcp-registry/servers-latest.jsonl';
const SHIELD = process.env.ATLAS_SHIELD || '/home/user/datasets/mcpshield-scan-2026/mcp-security-scan-2026.jsonl';

const PRIVATE_KEY_RE = /(private[_-]?key|seed[_-]?phrase|mnemonic|wallet)/i;
const PASSWORD_NAME = /password/i;
const DBURL_NAME = /(database_url|db_url|database_uri|db_uri|postgres_url|postgresql_url|mysql_url|mongo_url|mongodb_uri|mongodb_url|connection_string|conn_string|datasource_url|dsn|jdbc_url)/i;
const LLM_NAME = /(openai_(api_)?key|anthropic_(api_)?key|(gemini|google_genai|googleai)_api_key|mistral_api_key|cohere_api_key|groq_api_key|together_api_key|perplexity_api_key|pplx_api_key|hf_token|huggingface(hub)?_(api_)?(token|key)|replicate_api_token|openrouter_api_key|deepseek_api_key|xai_api_key|grok_api_key)/i;

// The auth scheme a secret header's declared value names ("Bearer {token}" -> "Bearer"),
// or '' when the record gives no value or the value is the bare secret ("{api_key}").
function schemeOf(value) {
  const m = /^\s*([A-Za-z][A-Za-z0-9._-]*)\s+\S/.exec(String(value || ''));
  return m ? m[1] : '';
}

function hostOf(u) {
  const m = /^[a-z]+:\/\/([^/]+)/i.exec(u || '');
  return m ? m[1].toLowerCase() : null;
}
function slugOf(repo) {
  if (!repo || repo.source !== 'github' || !repo.url) return null;
  const m = /github\.com\/([^/]+\/[^/#?]+)/i.exec(repo.url);
  if (!m) return null;
  return m[1].replace(/\.git$/, '');
}

async function loadGrades() {
  const byslug = new Map(); // lower slug -> {grade, score, findingCount, target, slug}
  const rl = createInterface({ input: createReadStream(SHIELD), crlfDelay: Infinity });
  for await (const line of rl) {
    if (!line.trim()) continue;
    const o = JSON.parse(line);
    byslug.set(String(o.slug).toLowerCase(), { grade: o.grade, score: o.score, findings: o.findingCount, tools: o.toolCount, slug: o.slug });
  }
  return byslug;
}

async function run() {
  const grades = await loadGrades();
  const serversOut = [];
  const bindingsOut = [];
  const credCount = new Map();   // class -> { servers:Set, examples:Set, host, bucket, kind }
  const stats = { servers: 0, publishers: new Set(), secret_servers: 0, secret_env: 0, unmarked: 0,
                  private_key: 0, password: 0, db_url: 0, llm_key: 0, since_oct: 0, secret_header_servers: 0,
                  graded_servers: 0, graded_slugs: new Set(), grade_dist: {}, remote_secret_header: 0 };

  const rl = createInterface({ input: createReadStream(REG), crlfDelay: Infinity });
  for await (const line of rl) {
    if (!line.trim()) continue;
    const o = JSON.parse(line);
    const s = o.server; if (!s || !s.name) continue;
    const meta = (o._meta && o._meta['io.modelcontextprotocol.registry/official']) || {};
    const name = s.name;
    const publisher = name.includes('/') ? name.split('/')[0] : name;
    stats.servers++; stats.publishers.add(publisher);
    const published = meta.publishedAt || '';
    if (published >= '2026-10-01') stats.since_oct++;

    const regTypes = [...new Set((s.packages || []).map(p => p.registryType).filter(Boolean))];
    const transports = new Set();
    const remoteHosts = [];
    let hasRemote = 0;
    for (const r of s.remotes || []) { hasRemote = 1; transports.add(r.type); const h = hostOf(r.url); if (h) remoteHosts.push(h); }
    for (const p of s.packages || []) { const t = p.transport && p.transport.type; if (t) transports.add(t); }

    // ---- credential bindings -------------------------------------------------
    const buckets = [];
    const classes = new Set();
    let hasSecret = false, nEnv = 0, nSecretEnv = 0, unmarked = 0;
    let wantPk = 0, wantPw = 0, wantDb = 0, wantLlm = 0, srvHeader = 0;
    const seenBinding = new Map();
    const addBinding = (cls, b) => {
      const id = `${name}|${cls}|${b.method}`;
      const seen = seenBinding.get(id);
      if (seen) { if (b.transport && !seen.transports.includes(b.transport)) seen.transports.push(b.transport); return; }
      // scheme / transports: what the record says about a secret header (its value's auth
      // scheme, e.g. "Bearer {token}", and the remote's transport), for Nodwyn's position
      // and methods (lib/nodwyn.mjs). Neither changes a count.
      const row = { id, src: name, dst: cls, publisher, class: cls, basis: b.basis,
        dest: b.host || '', method: b.method, bucket: b.bucket, kind: b.kind,
        rule: b.rule || '', is_secret: b.isSecret ? 1 : 0,
        scheme: b.scheme || '', transports: b.transport ? [b.transport] : [] };
      seenBinding.set(id, row);
      bindingsOut.push(row);
      classes.add(cls); buckets.push(b.bucket);
      let c = credCount.get(cls);
      if (!c) { c = { servers: new Set(), examples: new Set(), host: b.host || '', bucket: b.bucket, kind: b.kind }; credCount.set(cls, c); }
      c.servers.add(name);
      if (b.example) c.examples.add(b.example);
    };

    for (const p of s.packages || []) {
      for (const ev of p.environmentVariables || []) {
        const nm = ev.name || ''; const desc = ev.description || ''; const isSec = ev.isSecret === true;
        nEnv++;
        if (isSec) { nSecretEnv++; hasSecret = true; stats.secret_env++; }
        if (KEYISH_RE.test(nm) && !isSec) { unmarked++; stats.unmarked++; }
        if (PRIVATE_KEY_RE.test(nm)) wantPk = 1;
        if (PASSWORD_NAME.test(nm)) wantPw = 1;
        if (DBURL_NAME.test(nm)) wantDb = 1;
        if (LLM_NAME.test(nm)) wantLlm = 1;
        const b = classifyEnvVar(nm, desc, isSec);
        if (b) { b.isSecret = isSec; b.example = nm; addBinding(b.cls, b); }
      }
    }
    for (const r of s.remotes || []) {
      const h = hostOf(r.url);
      for (const hd of r.headers || []) {
        if (hd.isSecret === true) {
          hasSecret = true; srvHeader = 1;
          const b = classifyHeader(hd.name || 'Authorization', h || '');
          b.isSecret = true; b.example = hd.name || 'Authorization';
          b.scheme = schemeOf(hd.value); b.transport = r.type || '';
          addBinding(b.cls, b);
          stats.remote_secret_header++;
        }
      }
    }
    if (srvHeader) stats.secret_header_servers++;
    if (hasSecret) stats.secret_servers++;
    stats.private_key += wantPk; stats.password += wantPw; stats.db_url += wantDb; stats.llm_key += wantLlm;

    const primaryBucket = pickPrimaryBucket(buckets.length ? buckets : ['none']);

    // ---- grade join ----------------------------------------------------------
    const slug = slugOf(s.repository);
    let grade = '', score = null, gfindings = null;
    if (slug) {
      const g = grades.get(slug.toLowerCase());
      if (g) { grade = g.grade; score = g.score; gfindings = g.findings;
        stats.graded_servers++; stats.graded_slugs.add(slug.toLowerCase());
        stats.grade_dist[g.grade] = (stats.grade_dist[g.grade] || 0) + 0; }
    }

    serversOut.push({
      name, title: s.title || name.split('/').pop(), description: s.description || '',
      publisher, status: meta.status || 'active', published, version: s.version || '',
      reg_types: regTypes, transport: hasRemote && !regTypes.length ? 'remote' : [...transports][0] || 'remote',
      transports: [...transports], has_remote: hasRemote, remote_hosts: [...new Set(remoteHosts)],
      repo_slug: slug ? slug.toLowerCase() : '', website: s.websiteUrl || '',
      has_secret: hasSecret ? 1 : 0, n_env: nEnv, n_secret_env: nSecretEnv, unmarked_secretish: unmarked,
      wants_private_key: wantPk, wants_password: wantPw, wants_db_url: wantDb, wants_llm_key: wantLlm,
      cred_classes: [...classes], primary_class: primaryBucket,
      grade, score, grade_findings: gfindings,
      capability: '', // filled by measure.mjs
    });
  }

  // grade distribution over DISTINCT slugs (matches the registry join)
  const distinctGrade = {};
  { const seen = new Set();
    for (const r of serversOut) if (r.repo_slug && r.grade && !seen.has(r.repo_slug)) { seen.add(r.repo_slug); distinctGrade[r.grade] = (distinctGrade[r.grade] || 0) + 1; } }

  const creds = [...credCount.entries()].map(([cls, c]) => ({
    class: cls, label: (BUCKETS[c.bucket] || {}).label || cls, bucket: c.bucket, kind: c.kind,
    dest_host: c.host || '', servers: c.servers.size,
    example_names: [...c.examples].slice(0, 8),
  })).sort((a, b) => b.servers - a.servers);

  writeFileSync(path.join(DATA, 'atlas-servers.jsonl'), serversOut.map(r => JSON.stringify(r)).join('\n') + '\n');
  writeFileSync(path.join(DATA, 'atlas-bindings.jsonl'), bindingsOut.map(r => JSON.stringify(r)).join('\n') + '\n');
  writeFileSync(path.join(DATA, 'atlas-creds.json'), JSON.stringify(creds, null, 1));
  const statsOut = { ...stats, publishers: stats.publishers.size, graded_slugs: stats.graded_slugs.size,
    bindings: bindingsOut.length, distinct_grade: distinctGrade,
    basis: bindingsOut.reduce((a, b) => (a[b.basis] = (a[b.basis] || 0) + 1, a), {}) };
  writeFileSync(path.join(DATA, 'atlas-stats.json'), JSON.stringify(statsOut, null, 1));

  console.log('[prep] servers       ', statsOut.servers);
  console.log('[prep] publishers    ', statsOut.publishers);
  console.log('[prep] secret servers ', statsOut.secret_servers, ' secret env', statsOut.secret_env);
  console.log('[prep] unmarked keyish', statsOut.unmarked);
  console.log('[prep] private-key', stats.private_key, 'password', stats.password, 'db-url', stats.db_url, 'llm-key', stats.llm_key);
  console.log('[prep] since 1 Oct   ', statsOut.since_oct);
  console.log('[prep] remote secret-header servers', statsOut.secret_header_servers);
  console.log('[prep] graded servers', statsOut.graded_servers, ' distinct repos', statsOut.graded_slugs, distinctGrade);
  console.log('[prep] bindings      ', statsOut.bindings, statsOut.basis);
  console.log('[prep] credential classes', creds.length);
}
run().catch(e => { console.error(e); process.exit(1); });

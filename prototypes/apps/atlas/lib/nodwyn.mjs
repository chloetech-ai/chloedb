// MCP Atlas — bindings in Nodwyn's real format.
//
// Nodwyn's binding table (internal/binding/binding.go, at commit cdc30069) decodes [[bind]]
// rows with exactly class, dest, positions and methods (and optional scopes), refuses unknown
// keys and requires all four. This module writes those rows from what a server's registry
// record declares, the way Nodwyn's own Connections writes them when a key is added
// (internal/frontend/rpc/mcp.go):
//   - a remote server: class "<server>-key", dest = the URL's host, the secret header as the
//     position ("header:Authorization:Bearer" when the record names no scheme, as Nodwyn
//     defaults), methods ["POST"] (Streamable HTTP sends every message as a POST);
//   - a contained (stdio) server: dest = the provider's host, position "env:NAME", methods ["*"].
// The basis (exact / inferred) is a TOML comment on the row's [[bind]] line. An unknown
// binding gets no row: Nodwyn denies a secret-bearing egress no row binds (ADR 6).
// bindcheck/ runs Nodwyn's own Parse over every server's table at load.

export const NODWYN_COMMIT = 'cdc30069';
export const NODWYN_PARSER = 'Nodwyn binding.Parse (internal/binding/binding.go, cdc30069)';

// Nodwyn's built-in secret classes (internal/detect/l1/l1.go) for the providers it detects.
const L1_CLASS = { openai: 'openai-key', anthropic: 'anthropic-key', github: 'github-token', aws: 'aws-access-key', slack: 'slack-token' };

// Providers that serve each account on its own subdomain: the row binds "*.<domain>" (a
// wildcard Nodwyn's table matches, binding.go hostMatches) since the record cannot name the
// account's host.
const TENANT = new Set(['amazonaws.com', 'googleapis.com', 'azure.com', 'supabase.co', 'pinecone.io']);

// The host a row can bind, from a destination as prep recorded it. Returns
// { host, wildcard, why }: host '' (with why) when there is no host name to bind.
//   "mcp.apify.com?tools=x" -> "mcp.apify.com"; "jarvis.ts.net:8443" -> "jarvis.ts.net"
//   "{tenant}.example.com"  -> "*.example.com" (the URL names the account as a variable)
//   "{host}", "database"    -> '' (the host is a variable, or in the value, not the record)
const TEMPLATED = /[{}$<>%]/;
export function bindHost(dest, basis) {
  let h = String(dest || '').toLowerCase().split(/[?#/]/)[0];
  const ipv6 = h.startsWith('[');
  if (!ipv6) h = h.replace(/:\d+$/, '');
  const labels = h.split('.');
  let last = -1;
  labels.forEach((l, i) => { if (TEMPLATED.test(l)) last = i; });
  let wildcard = false;
  if (last >= 0) { labels.splice(0, last + 1); wildcard = true; }
  if (wildcard && labels.length < 2) {
    return { host: '', wildcard: false, why: 'the host in the record\'s URL is a variable the user fills in' };
  }
  h = labels.join('.');
  if (!wildcard && TENANT.has(h)) wildcard = true;
  const name = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$/.test(h);
  const ip4 = /^\d{1,3}(\.\d{1,3}){3}$/.test(h);
  if (!name && !ip4) {
    return { host: '', wildcard: false, why: basis === 'inferred'
      ? 'the host is in the variable\'s value, not in the record'
      : 'the record\'s URL has no host name a row can bind' };
  }
  return { host: wildcard ? '*.' + h : h, wildcard, why: '' };
}

// A Nodwyn server name (internal/config/mcp.go: ^[a-z][a-z0-9-]{0,31}$) from a registry name.
const GENERIC = new Set(['mcp', 'server', 'mcp-server', 'api', 'remote', 'mcp-remote']);
export function nodwynName(registryName = '') {
  const [pub, ...rest] = String(registryName).split('/');
  let seg = rest.length ? rest.join('-') : pub;
  if (GENERIC.has(seg.toLowerCase())) {
    const labels = pub.split('.').filter(l => !['com', 'io', 'ai', 'dev', 'app', 'net', 'org', 'co', 'github'].includes(l));
    seg = `${labels.pop() || pub}-${seg}`;
  }
  let s = seg.toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-').replace(/-+/g, '-').replace(/^-|-$/g, '');
  if (!/^[a-z]/.test(s)) s = 'mcp-' + s;
  s = s.slice(0, 32).replace(/-+$/, '');
  return s || 'mcp';
}

const str = v => JSON.stringify(String(v));                 // a TOML basic string
const arr = a => `[${a.map(str).join(', ')}]`;
const note = s => String(s).replace(/[\r\n]+/g, ' ');       // a comment stays on its line

function methodsFor(b) {
  if (b.kind !== 'header') return ['*'];
  const t = new Set(b.transports || []);
  if (t.has('sse')) return ['GET', 'POST'];                  // SSE: a GET stream plus POSTed messages
  if (t.has('streamable-http')) return ['POST'];
  return ['*'];
}

// One binding -> { row: {class, dest, positions, methods} | null, toml, why }.
// usedClasses keeps a server's classes distinct when it declares two secret headers.
export function bindRow(serverName, b, usedClasses = new Map()) {
  const varname = String(b.method || '').replace(/^env:|^header:/, '');
  if (b.basis === 'unknown') {
    const why = 'no destination is known';
    return { row: null, why, toml: `# ${note(varname)} (${note(b.class)}): unknown destination, so no row. Nodwyn denies unbound egress by default (ADR 6).` };
  }
  const { host: dest, wildcard, why } = bindHost(b.dest, b.basis);
  if (!dest) {
    return { row: null, why, toml: `# ${note(varname)} (${note(b.class)}, ${b.basis}): ${why}, so no row. Nodwyn denies unbound egress by default (ADR 6).` };
  }

  let cls = b.class === 'bearer' ? `${nodwynName(serverName)}-key` : (L1_CLASS[b.class] || `${b.class}-key`);
  const pos = (() => {
    if (b.kind !== 'header') return String(b.method);
    const scheme = b.scheme || (varname.toLowerCase() === 'authorization' ? 'Bearer' : '');
    return scheme ? `header:${varname}:${scheme}` : `header:${varname}`;
  })();
  // a second, different secret of the same class on one server gets its own class name
  const prev = usedClasses.get(cls);
  if (prev && prev !== pos) { let i = 2; while (usedClasses.has(`${cls}-${i}`)) i++; cls = `${cls}-${i}`; }
  usedClasses.set(cls, pos);

  const methods = methodsFor(b);
  let basis;
  if (b.basis === 'exact') {
    const sch = b.kind !== 'header' ? '' : b.scheme ? `, scheme ${b.scheme} from the record`
      : varname.toLowerCase() === 'authorization' ? ', scheme Bearer assumed (Nodwyn\'s default; the record names none)' : '';
    basis = `basis = exact: the record's remote URL and its secret header ${varname}${sch}`;
  } else {
    basis = `basis = inferred: ${varname} matches /${b.rule}/`;
  }
  if (wildcard) basis += TENANT.has(dest.slice(2)) ? '; the provider serves each account on a subdomain'
    : `; the URL names the account's subdomain as a variable (${note(String(b.dest).split(/[?#/]/)[0])})`;
  const row = { class: cls, dest, positions: [pos], methods };
  const toml = [`[[bind]] # ${note(basis)}`, `class = ${str(row.class)}`, `dest = ${str(row.dest)}`,
    `positions = ${arr(row.positions)}`, `methods = ${arr(row.methods)}`].join('\n');
  return { row, toml, why: '' };
}

// The file the card shows and the parser checks: a header, then each binding's text.
export function joinToml(serverName, pieces) {
  const head = `# bindings.toml for ${note(serverName)}, generated by MCP Atlas from its registry record.\n` +
    `# Checked with Nodwyn's own parser (internal/binding/binding.go at ${NODWYN_COMMIT}).`;
  return [head, ...pieces].join('\n\n') + '\n';
}

// A server's whole bindings.toml, in the order its rows are checked and shown.
export function serverToml(serverName, bindings) {
  const sorted = [...bindings].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const used = new Map();
  const parts = sorted.map(b => ({ b, ...bindRow(serverName, b, used) }));
  const toml = joinToml(serverName, parts.map(p => p.toml));
  return { toml, parts, want: parts.filter(p => p.row).map(p => p.row) };
}

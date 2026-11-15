// Who may read and write an app's tables, declared with the app and enforced
// by the engine on every read and write its people make (engine.mjs): the
// screens do not decide it, the server does.
//
// A table's access is a list of ALLOW statements, kept in the table's spec
// (its capability, so a version of the app carries the access it had):
//
//   ALLOW READ ON products TO everyone
//   ALLOW READ, WRITE ON receivables TO members
//   ALLOW ALL ON notes TO members WHERE owner_id = :user_id
//   ALLOW ALL ON invoices TO members WHERE company_id = :company_id
//   ALLOW ALL ON invoices TO admins
//
// - What: READ, ADD (insert), CHANGE (update), REMOVE (delete), WRITE (the
//   three writes) or ALL, several with commas.
// - Who: EVERYONE (visitors too), MEMBERS (anyone signed in to the app),
//   ADMINS (its admins and its owner) or OWNER. Each includes the ones
//   above it: a statement TO members also lets admins and the owner.
// - Which rows (optional): WHERE <column> = :user_id (the row is the
//   signed-in person's) or <column> = :company_id (the row is the company the
//   person works in now, from the memberships the engine keeps, see
//   services.mjs), several joined by OR. A read sees those rows only; an add
//   fills the column when it is missing and is refused when it names someone
//   else; a change or remove reaches those rows only and cannot move a row
//   out of them.
//
// A table with no statement is as tables were before access was declared:
// whoever reaches the app (as its publish choice says) reaches the table. A
// table with one statement or more lets only what its statements allow.
// The studio (the app's builder and owner, not through the public path)
// always has every row: the statements are for the app's people.
//
// Server functions are the app's own code. One the app declares callers for
//
//   ALLOW CALL ON place-order TO everyone
//
// may be called by those people (a call from anyone else is refused, 401 or
// 403, said to the person), and then runs with the app's rights, as a
// stored procedure with SECURITY DEFINER does: a visitor's order lowers the
// stock no visitor may change. A function with no CALL statement runs as
// the person who called it (their reads and writes held to the tables'
// statements). Triggers and schedules run as the app, since they answer a
// write that was allowed or the clock, not a person.

export const ACTIONS = ['read', 'add', 'change', 'remove'];
const ACTION_WORDS = { READ: ['read'], SELECT: ['read'], ADD: ['add'], INSERT: ['add'], CHANGE: ['change'], UPDATE: ['change'], EDIT: ['change'],
  REMOVE: ['remove'], DELETE: ['remove'], WRITE: ['add', 'change', 'remove'], ALL: ACTIONS };
export const WHO = ['everyone', 'members', 'admins', 'owner'];
const WHO_WORDS = { EVERYONE: 'everyone', PUBLIC: 'everyone', VISITORS: 'everyone', ANYONE: 'everyone', MEMBERS: 'members', MEMBER: 'members', USERS: 'members',
  'SIGNED IN': 'members', 'SIGNED-IN': 'members', ADMINS: 'admins', ADMIN: 'admins', OWNER: 'owner', OWNERS: 'owner' };
// A person's role in an app, from the least: someone not signed in, a member,
// an admin, the owner.
export const ROLES = ['visitor', 'member', 'admin', 'owner'];
const rank = (role) => Math.max(0, ROLES.indexOf(role));
const NAME = /^[a-z][a-z0-9_]*$/;
const BY = { user_id: 'user', 'user.id': 'user', company_id: 'company', 'company.id': 'company' };
// The parameters the engine binds itself: what a scope compares with.
export const SCOPE_PARAM = { user: 'sa_user_id', company: 'sa_company_id' };

const fail = (msg) => { throw Object.assign(new Error(`ALLOW: ${msg}`), { status: 400 }); };
const HOW = 'ALLOW READ | ADD | CHANGE | REMOVE | WRITE | ALL ON <table> TO everyone | members | admins | owner [WHERE <column> = :user_id | <column> = :company_id | <column> IN (SELECT <column> FROM <table> WHERE <column> = :user_id)]';
// A scope through a link table: the rows whose column is one of the values
// the person's own rows of another table hold (the accounts they are on).
const VIA = /^([A-Za-z_][A-Za-z0-9_]*)\s+IN\s*\(\s*SELECT\s+([A-Za-z_][A-Za-z0-9_]*)\s+FROM\s+([A-Za-z_][A-Za-z0-9_]*)\s+WHERE\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*:([A-Za-z_.]+)\s*\)$/i;
export const viaKey = (v) => `${v.table}.${v.pick}.${v.match}.${v.by}`;

// One statement -> {table, actions, who, scope: [{col, by}]}.
export function parseAllow(text) {
  const s = String(text || '').trim().replace(/;\s*$/, '').replace(/\s+/g, ' ');
  // ALLOW CALL ON <function> TO <who>: who may call one of the app's functions
  const c = s.match(/^ALLOW\s+CALL\s+ON\s+([A-Za-z][A-Za-z0-9_-]*)\s+TO\s+(.+)$/i);
  if (c) {
    const who = WHO_WORDS[c[2].trim().toUpperCase().replace(/^THE\s+(APP'?S\s+)?/, '').replace(/\s+ONLY$/, '')];
    if (!who) fail(`TO ${c[2].trim()}: say everyone, members, admins or owner`);
    return { table: c[1].toLowerCase(), actions: ['call'], who, scope: [], call: true };
  }
  const m = s.match(/^ALLOW\s+(.+?)\s+ON\s+([A-Za-z_][A-Za-z0-9_]*)\s+TO\s+(.+?)(?:\s+WHERE\s+(.+))?$/i);
  if (!m) fail(`write it as ${HOW}, not ${JSON.stringify(s.slice(0, 120))}`);
  const actions = new Set();
  for (const w of m[1].split(/\s*,\s*|\s+AND\s+/i)) {
    const a = ACTION_WORDS[w.trim().toUpperCase()];
    if (!a) fail(`${w.trim()} is not READ, ADD, CHANGE, REMOVE, WRITE or ALL`);
    for (const x of a) actions.add(x);
  }
  const table = m[2].toLowerCase();
  if (!NAME.test(table)) fail(`${m[2]} is not a table name`);
  const who = WHO_WORDS[m[3].trim().toUpperCase().replace(/^THE\s+(APP'?S\s+)?/, '').replace(/\s+ONLY$/, '')];
  if (!who) fail(`TO ${m[3].trim()}: say everyone, members, admins or owner`);
  const scope = [];
  if (m[4]) {
    for (const term of m[4].split(/\s+OR\s+/i)) {
      const via = term.trim().match(VIA);
      if (via) {
        const by = BY[via[5].toLowerCase()];
        if (!by) fail(`WHERE ... IN (SELECT ... WHERE <column> = :user_id or :company_id), not :${via[5]}`);
        const col = via[1].toLowerCase();
        if (col === 'created') fail(`WHERE ${col}: scope rows by a column that holds what the person belongs to (an account's id)`);
        scope.push({ col, by, via: { table: via[3].toLowerCase(), pick: via[2].toLowerCase(), match: via[4].toLowerCase(), by } });
        continue;
      }
      const t = term.trim().replace(/^\((.*)\)$/, '$1').trim().match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*:([A-Za-z_.]+)$/);
      if (!t || !BY[t[2].toLowerCase()]) fail(`WHERE takes <column> = :user_id, <column> = :company_id or <column> IN (SELECT <column> FROM <table> WHERE <column> = :user_id) (joined by OR), not ${JSON.stringify(term.trim())}`);
      const col = t[1].toLowerCase();
      if (col === 'id' || col === 'created') fail(`WHERE ${col}: scope rows by a column that holds a person's or a company's id (owner_id, company_id)`);
      scope.push({ col, by: BY[t[2].toLowerCase()] });
    }
  }
  return { table, actions: ACTIONS.filter((a) => actions.has(a)), who, scope };
}

// The statement as the engine keeps and shows it.
export function allowText(rule) {
  if (rule.call) return `ALLOW CALL ON ${rule.table} TO ${rule.who}`;
  const what = rule.actions.length === ACTIONS.length ? 'ALL' : rule.actions.map((a) => a.toUpperCase()).join(', ');
  const where = rule.scope.length ? ` WHERE ${rule.scope.map((t) => (t.via ? `${t.col} IN (SELECT ${t.via.pick} FROM ${t.via.table} WHERE ${t.via.match} = :${t.by}_id)` : `${t.col} = :${t.by}_id`)).join(' OR ')}` : '';
  return `ALLOW ${what} ON ${rule.table} TO ${rule.who}${where}`;
}

// The statements of a plan's "access" (a list, or text with one per line or
// separated by ;), grouped by table: Map table -> [rule].
export function parseAccessList(list) {
  const items = Array.isArray(list) ? list : String(list || '').split(/;|\n/);
  const out = new Map();
  for (const raw of items) {
    if (!String(raw || '').trim()) continue;
    const r = parseAllow(raw);
    if (!out.has(r.table)) out.set(r.table, []);
    out.get(r.table).push(r);
  }
  return out;
}

// Problems with a plan's access statements, against the app's tables and
// columns (Map table -> Set of columns).
export function checkAccess(list, columns, functions = null) {
  const problems = [];
  const items = Array.isArray(list) ? list : list ? [list] : [];
  for (const raw of items) {
    let r;
    try { r = parseAllow(raw); } catch (e) { problems.push(`access: ${e.message}`); continue; }
    if (r.call) { if (functions && !functions.has(r.table)) problems.push(`access: ${allowText(r)} names the function ${r.table}, which the app does not have`); continue; }
    if (!columns.has(r.table)) { problems.push(`access: ${allowText(r)} names the table ${r.table}, which the app does not have`); continue; }
    for (const t of r.scope) {
      if (!columns.get(r.table).has(t.col)) problems.push(`access: ${allowText(r)}: ${r.table} has no column ${t.col} (add it to the table, a keyword column)`);
      if (!t.via) continue;
      if (!columns.has(t.via.table)) problems.push(`access: ${allowText(r)} reads the table ${t.via.table}, which the app does not have (the table linking people to what they belong to)`);
      else for (const c of [t.via.pick, t.via.match]) if (!columns.get(t.via.table).has(c)) problems.push(`access: ${allowText(r)}: ${t.via.table} has no column ${c}`);
    }
  }
  return problems;
}

const compiled = new Map();
// A table spec's rules (spec.access holds the statements), or null when the
// table declares none.
export function rulesOf(spec) {
  const list = spec && Array.isArray(spec.access) ? spec.access : null;
  if (!list || !list.length) return null;
  const key = list.join('\n');
  let rules = compiled.get(key);
  if (!rules) {
    rules = [];
    for (const s of list) { try { rules.push(parseAllow(s)); } catch { /* a statement that no longer parses allows nothing */ } }
    if (compiled.size > 2000) compiled.clear();
    compiled.set(key, rules);
  }
  return rules;
}

const PEOPLE = { everyone: 'everyone', members: "the app's signed-in members", admins: "the app's admins", owner: "the app's owner" };
const VERB = { read: 'see', add: 'add to', change: 'change', remove: 'remove from' };
const refusal = (status, message) => Object.assign(new Error(message), { status, refused: status === 401 ? 'sign-in' : 'access' });

// Whether a person may call a function whose callers are `callers` (a WHO
// word): nothing when they may, else a thrown 401 (sign in first) / 403.
export function mayCall(callers, title, who) {
  if (!callers || !who) return;
  const need = WHO.indexOf(callers);
  if (rank(who.role) >= need) return;
  if (!who.user) throw refusal(401, `Sign in first: only ${PEOPLE[callers]} can ${title}`);
  throw refusal(403, `Only ${PEOPLE[callers]} can ${title}`);
}

// What `who` may do: {all: true} (every row), {scope: [{col, by}]} (those
// rows), or a thrown 401 (sign in first) / 403. who is {role, user, company}
// (a person of the app, or role visitor and no user); null is the studio.
export function decide(spec, action, who) {
  const rules = rulesOf(spec);
  if (!rules || !who) return { all: true };
  const table = spec.name;
  const mine = rules.filter((r) => r.actions.includes(action));
  const fits = mine.filter((r) => rank(who.role) >= WHO.indexOf(r.who));
  if (!fits.length) {
    const least = mine.length ? mine.map((r) => WHO.indexOf(r.who)).sort()[0] : -1;
    const whom = least >= 0 ? PEOPLE[WHO[least]] : null;
    if (!who.user) throw refusal(401, `Sign in first: ${whom ? `only ${whom} can ${VERB[action]} ${table}` : `${table} is not open to visitors`}`);
    throw refusal(403, whom ? `Only ${whom} can ${VERB[action]} ${table}` : `Nobody can ${VERB[action]} ${table} here`);
  }
  if (fits.some((r) => !r.scope.length)) return { all: true };
  const terms = [];
  const same = (x, t) => x.col === t.col && x.by === t.by && (x.via ? !!t.via && viaKey(x.via) === viaKey(t.via) : !t.via);
  for (const r of fits) for (const t of r.scope) if (!terms.some((x) => same(x, t))) terms.push(t.via ? { ...t, values: (who.links && who.links[viaKey(t.via)]) || [] } : t);
  const usable = terms.filter((t) => (t.by === 'user' ? !!who.user : !!who.company));
  if (!usable.length) {
    if (!who.user) throw refusal(401, `Sign in first: ${table} shows each person their own rows`);
    throw refusal(403, `Join or choose a company first: ${table} holds each company's own rows`);
  }
  return { scope: usable };
}

const valueOf = (t, who) => (t.by === 'user' ? who.user?.id : who.company);

// The scope as SQL on a table's alias, with the engine's own parameters.
const lit = (v) => `'${String(v).replace(/'/g, "''")}'`;
export function scopeSql(scope, alias, { left = false } = {}) {
  // (a link table's values are known before the query: the person's own)
  const terms = scope.map((t) => (t.via ? (t.values && t.values.length ? `${alias}.${t.col} IN (${t.values.map(lit).join(', ')})` : '1 = 0') : `${alias}.${t.col} = :${SCOPE_PARAM[t.by]}`)).join(' OR ');
  return left ? `(${terms} OR ${alias}.id IS NULL)` : `(${terms})`;
}

// The engine's parameters for a scope: never what the caller sent.
export function scopeParams(who) {
  const out = {};
  if (who?.user?.id) out[SCOPE_PARAM.user] = String(who.user.id);
  if (who?.company) out[SCOPE_PARAM.company] = String(who.company);
  return out;
}

// (a keyword[] column is theirs when any of its values is)
export const inScope = (row, scope, who) => !!row && scope.some((t) => {
  const v = row[t.col];
  if (v == null) return false;
  if (t.via) { const ok = new Set((t.values || []).map(String)); return Array.isArray(v) ? v.some((x) => ok.has(String(x))) : ok.has(String(v)); }
  return Array.isArray(v) ? v.map(String).includes(String(valueOf(t, who))) : String(v) === String(valueOf(t, who));
});

// The link tables an app's access reads, each once: [{table, pick, match, by}].
export function linksOf(specs) {
  const out = new Map();
  for (const spec of specs) for (const r of rulesOf(spec) || []) for (const t of r.scope) if (t.via) out.set(viaKey(t.via), t.via);
  return [...out.values()];
}

// An add under a scope: the scope's columns the row leaves out are filled
// with the person's (or company's) id; a row that names someone else is
// refused.
export function fillScope(fields, scope, who, table) {
  const out = { ...(fields || {}) };
  // (what a person belongs to is theirs to name: a link scope fills nothing)
  for (const t of scope) if (!t.via && (out[t.col] === undefined || out[t.col] === null || out[t.col] === '')) out[t.col] = valueOf(t, who);
  if (!inScope(out, scope, who)) throw refusal(403, `You can add to ${table} only as yourself (${scope.map((t) => t.col).join(' or ')})`);
  return out;
}

// Whether a request asks that the data be for signed-in people, roles or
// companies only (the forge's check that the plan declares access).
export const WANTS_ACCESS = /\b(only (?:signed[- ]?in|logged[- ]?in|registered|authenticated|members|admins?|the owner)|sign[- ]?in (?:is )?(?:required|first|only)|(?:must|have to|need to|should) (?:be )?(?:sign(?:ed)?[- ]?in|log(?:ged)?[- ]?in)|require[sd]? (?:a )?(?:sign[- ]?in|log[- ]?in|login|authentication|an account)|add (?:user )?(?:authentication|auth|login|sign[- ]?in)|authentication|authenticated|roles?\b|admins? (?:can|only|may)|per[- ](?:company|workspace|team|organi[sz]ation|tenant)|each (?:company|workspace|team|organi[sz]ation|tenant)|multi[- ]?tenant|their own (?:rows|data|records|notes|items|entries)|private to (?:each|the) (?:user|person))/i;
export const wantsAccess = (text) => WANTS_ACCESS.test(String(text || ''));

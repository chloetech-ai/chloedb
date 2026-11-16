// Who called, beside a cluster: chloedb-server forwards /v1/apps/apps/<path>
// here with its app token and says who the caller is in headers it sets
// itself (x-chloedb-principal, x-chloedb-scope and, for a key limited to
// tables, x-chloedb-tables: `app_*=admin`), which the engine trusts because
// they come with the token. The engine's app.json says it checks its
// callers ("checks_callers": true), so the server forwards it keys limited
// to tables, and it refuses what they may not do, by the tables of each
// app (app_<id>_*, store.mjs):
//
// - reading an app (its screens, data, versions, usage) takes read on its
//   tables; using it as its screens do (rows written, functions run, files)
//   takes write; building, changing, publishing, renaming, deleting it and
//   switching its powers on take admin there;
// - building a new app (its id is not known yet) takes admin on every
//   app's tables (`app_*`), as the app-owner preset gives;
// - what is not one app's (the studio's login, the graph of every app's
//   pieces) is not for a key limited to tables.
//
// A key of every table keeps what it had: read to look, admin to change.
// A request that did not come through chloedb-server (the studio on a
// laptop, a published app's visitors) is not a caller here: publish.mjs and
// login.mjs judge those.

const SCOPES = ['feed', 'read', 'write', 'admin'];
const level = (s) => Math.max(0, SCOPES.indexOf(String(s || '').trim().toLowerCase()));

export function callerOf(req) {
  const h = req.headers;
  if (!h['x-chloedb-scope']) return null;
  const tables = String(h['x-chloedb-tables'] || '').trim();
  return {
    name: String(h['x-chloedb-principal'] || 'someone'),
    scope: level(h['x-chloedb-scope']),
    grants: tables ? tables.split(',').filter((g) => g.trim()).map((g) => {
      const [t, s] = g.split('=');
      return { table: t.trim().toLowerCase(), scope: level(s ?? 'read') };
    }) : null,
  };
}

// What a caller may do to every table that starts with `prefix`: the most a
// grant gives that covers them all (`*`, or a prefix of the prefix).
export function onPrefix(c, prefix) {
  if (!c.grants) return c.scope;
  let best = 0;
  for (const g of c.grants) {
    const p = g.table.endsWith('*') ? g.table.slice(0, -1) : null;
    if (p !== null && prefix.startsWith(p)) best = Math.max(best, g.scope);
  }
  return best;
}

// The routes of one app that use it as its screens do: write on its tables.
const USE = new Set(['sql', 'insert', 'update', 'remove', 'call', 'files', 'files/info', 'track', 'error', 'signup', 'signin']);
// Routes of no one app that a key limited to tables may call.
const OPEN_GET = [/^\/app\.json$/, /^\/api\/status$/, /^\/api\/kit$/, /^\/api\/library$/, /^\/api\/geocode$/, /^\/api\/apps$/, /^\/api\/events$/, /^\/cc\/view\.js$/];

// Why `c` may not make this request (a 403's message), or null.
export function refusal(c, method, url) {
  if (!c) return null;
  const path = url.pathname;
  const get = method === 'GET' || method === 'HEAD';
  const need = (n, what) => `${method} ${path} needs the ${SCOPES[n]} scope${what}; the key ${JSON.stringify(c.name)} ${c.grants ? `is limited to ${c.grants.map((g) => `${g.table} (${SCOPES[g.scope]})`).join(', ')}` : `has the ${SCOPES[c.scope]} scope`}`;
  // A key of every table: read to look, admin for the rest, as before the
  // engine checked its callers.
  if (!c.grants) return get ? (c.scope >= 1 ? null : need(1, '')) : c.scope >= 3 ? null : need(3, '');
  if (!path.startsWith('/api/')) return get ? null : need(3, ' (not one app\'s)');
  const m = path.match(/^\/api\/apps\/([a-z0-9]+)(?:\/(.*))?$/);
  if (m) {
    const prefix = `app_${m[1]}_`;
    const rest = m[2] || '';
    const n = get ? 1 : USE.has(rest) ? 2 : 3;
    if (onPrefix(c, prefix) < n) return need(n, ` on the app's tables (${prefix}*)`);
    // A copy is a new app: as building one.
    if (rest === 'duplicate' && onPrefix(c, 'app_') < 3) return need(3, ' on every app\'s tables (app_*), to make a new app');
    return null;
  }
  if (method === 'POST' && path === '/api/apps') return onPrefix(c, 'app_') >= 3 ? null : need(3, ' on every app\'s tables (app_*), to build a new app');
  if (method === 'POST' && path === '/api/signout') return null;
  if (get && OPEN_GET.some((re) => re.test(path))) {
    if (path === '/api/events') {
      const app = url.searchParams.get('app');
      if (!app || !/^[a-z0-9]+$/.test(app) || onPrefix(c, `app_${app}_`) < 1) return `a key limited to tables follows one app's events (?app=<id>), of an app whose tables it reads; the key ${JSON.stringify(c.name)} names ${app ? `app ${app}, which it does not read` : 'none'}`;
    }
    return null;
  }
  return `${method} ${path} is not for a key limited to tables (it is not one app's)`;
}

// Whether `c` sees app `id` in a listing.
export function sees(c, id) {
  return !c || onPrefix(c, `app_${id}_`) >= 1;
}

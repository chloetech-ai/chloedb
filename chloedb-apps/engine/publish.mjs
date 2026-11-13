// Apps open to the public. The studio is for the people who build apps; an
// app its owner publishes is also open to everyone, at an address of its own:
//
// - /a/<id> on the engine's own address, at once;
// - <name>.<domain> when the engine has a domain (STRATA_APPS_DOMAIN, the
//   operator's spec.apps.domain, a wildcard DNS record pointing at it), the
//   app at /, as people expect of a site;
// - any host of the owner's (a CNAME to <name>.<domain>), listed with it.
//
// What the public reaches is the app as it runs, nothing of how it is made:
// its page and the runtime's files, the app's state without its requests,
// versions or people (publicState in server.mjs), and the calls its screens
// make (reads and writes of its own tables, its functions, signing up and in,
// files, and app.ask, app.fetch and app.email under the app's switches).
// Building, changing, other apps and the library stay behind the studio's
// login. Each visitor is held to a rate a minute, lower for what costs
// something (the model, the web, mail, accounts, uploads); app.ask is held
// to the app's own model calls a day too (reach.mjs), as it is for anyone.
//
// The choice is a row of sa_settings (the app, "publish"):
// {"on": true, "name": "kids-reading", "hosts": ["books.example.org"]}.

const NAME = /^[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?$/;
const HOST = /^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/;
// Names an app may not take: they would read as the service's own.
const RESERVED = new Set(['www', 'api', 'app', 'apps', 'studio', 'admin', 'login', 'mail', 'static', 'assets', 'cdn', 'status', 'docs', 'help', 'chloedb', 'strata']);

// A name for an app's address from its title: "Kids' reading" -> kids-reading.
export function nameFor(title) {
  const n = String(title || '').toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '')
    .replace(/['’]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40).replace(/-+$/, '');
  return n && !RESERVED.has(n) ? n : `app-${n || 'x'}`.slice(0, 40).replace(/-+$/, '');
}

// The routes a published app's page uses, and only those (method, path
// after /api/apps/<id>). GET /api/apps/<id> itself answers publicState.
const APP_CALLS = [
  ['GET', ''], ['GET', '/me'], ['GET', '/people'],
  ['POST', '/sql'], ['POST', '/insert'], ['POST', '/update'], ['POST', '/remove'], ['POST', '/call'],
  ['POST', '/signup'], ['POST', '/signin'], ['POST', '/files'], ['POST', '/files/info'],
  ['POST', '/track'], ['POST', '/error'], ['POST', '/email'], ['POST', '/ask'], ['POST', '/fetch'],
];
// Calls that cost something (the model, the network, mail, a new account):
// a lower rate per visitor.
const DEAR = new Set(['/ask', '/fetch', '/email', '/signup', '/signin', '/files']);
// The runtime's files, the same for every app and nothing private.
const FILES = new Set(['/studio.js', '/studio.css', '/activity.css', '/frame.js', '/screen.js', '/front.js', '/front.css', '/graph.js', '/kit.css', '/design.js', '/policy.js', '/activity.js']);

export class Publish {
  constructor({ store, domain, scheme, log = () => {}, rate = {} }) {
    this.store = store;
    this.domain = String(domain || '').toLowerCase().replace(/^\*?\./, '').replace(/\.$/, '') || null;
    if (this.domain && !HOST.test(this.domain)) throw new Error(`STRATA_APPS_DOMAIN is a domain name, like apps.example.com, not ${JSON.stringify(domain)}`);
    // How browsers reach <name>.<domain>: https behind the operator's
    // certificates, http on a domain served without them.
    this.scheme = /^https?$/.test(scheme || '') ? scheme : 'https';
    this.log = log;
    this.apps = new Map(); // app id -> {on, name, hosts}
    this.byHost = new Map(); // host -> app id
    this.loaded = 0;
    this.rate = { calls: rate.calls ?? Number(process.env.STRATA_APPS_PUBLIC_RATE || 240), dear: rate.dear ?? Number(process.env.STRATA_APPS_PUBLIC_DEAR_RATE || 20) };
    this.buckets = new Map(); // visitor -> {calls, dear, at}
  }

  // Every app's choice, read again at most every 15 s (another pod may have changed one).
  async load(force = false) {
    if (!force && Date.now() - this.loaded < 15000) return;
    this.loaded = Date.now();
    const rows = await this.store.db.query(`SELECT app, value FROM sa_settings WHERE name = 'publish' LIMIT 5000`).catch(() => []);
    this.apps.clear();
    for (const r of rows) { try { this.apps.set(r.app, JSON.parse(r.value)); } catch { /* a broken row publishes nothing */ } }
    this.index();
  }

  index() {
    this.byHost.clear();
    for (const [id, p] of this.apps) {
      if (!p.on) continue;
      if (this.domain && p.name) this.byHost.set(`${p.name}.${this.domain}`, id);
      for (const h of p.hosts || []) this.byHost.set(h, id);
    }
  }

  // What the studio and the app creator show: the switch and the addresses.
  of(appId) {
    const p = this.apps.get(appId) || { on: false, name: null, hosts: [] };
    return {
      on: !!p.on, name: p.name || null, hosts: p.hosts || [], domain: this.domain,
      urls: p.on ? [...(this.domain && p.name ? [`${this.scheme}://${p.name}.${this.domain}/`] : []), ...(p.hosts || []).map((h) => `https://${h}/`)] : [],
    };
  }

  // POST /api/apps/<id>/publish {on, name, hosts}: only from the studio or
  // the command center (the login checks that first).
  async set(appId, title, { on, name, hosts } = {}) {
    await this.load(true);
    const cur = this.apps.get(appId) || { on: false, name: null, hosts: [] };
    const next = { ...cur };
    if (on !== undefined) next.on = !!on;
    if (name !== undefined) {
      const n = String(name || '').trim().toLowerCase();
      if (!NAME.test(n)) throw Object.assign(new Error('a name is 1 to 40 letters, digits and dashes, starting and ending with a letter or a digit'), { status: 400 });
      if (RESERVED.has(n)) throw Object.assign(new Error(`"${n}" is kept for the service itself; pick another name`), { status: 400 });
      next.name = n;
    }
    if (hosts !== undefined) {
      const list = [...new Set((Array.isArray(hosts) ? hosts : []).map((h) => String(h).trim().toLowerCase().replace(/\.$/, '')).filter(Boolean))];
      const bad = list.find((h) => !HOST.test(h) || (this.domain && (h === this.domain || h.endsWith(`.${this.domain}`))));
      if (bad) throw Object.assign(new Error(`${bad} is not a host of your own (a name like books.example.org, outside ${this.domain || 'the engine’s domain'})`), { status: 400 });
      if (list.length > 5) throw Object.assign(new Error('an app has at most 5 hosts of its own'), { status: 400 });
      next.hosts = list;
    }
    if (!next.name) next.name = this.free(nameFor(title), appId);
    for (const [id, p] of this.apps) {
      if (id === appId) continue;
      if (p.name === next.name) throw Object.assign(new Error(`another app is called ${next.name}; pick another name`), { status: 409 });
      const taken = (next.hosts || []).find((h) => (p.hosts || []).includes(h));
      if (taken) throw Object.assign(new Error(`another app has ${taken}`), { status: 409 });
    }
    await this.store.putSetting(appId, 'publish', JSON.stringify(next));
    this.apps.set(appId, next);
    this.index();
    this.log(`${appId}: ${next.on ? `published as ${next.name}` : 'not published'}`);
    return this.of(appId);
  }

  free(base, appId) {
    const taken = new Set([...this.apps].filter(([id]) => id !== appId).map(([, p]) => p.name));
    if (!taken.has(base)) return base;
    for (let i = 2; ; i++) { const n = `${base.slice(0, 36)}-${i}`; if (!taken.has(n)) return n; }
  }

  // The published app a request's host names, or null.
  appForHost(req) {
    const host = String(req.headers.host || '').toLowerCase().replace(/:\d+$/, '').replace(/\.$/, '');
    return this.byHost.get(host) || null;
  }

  published(appId) { return !!this.apps.get(appId)?.on; }

  // Whether a request someone not signed in to the studio made is one the
  // public may make: {app, page} (page: the app's page itself), or null.
  async allows(req, url) {
    await this.load();
    const p = url.pathname;
    const hostApp = this.appForHost(req);
    if (req.method === 'GET' && FILES.has(p)) return { app: hostApp, file: true };
    if (url.searchParams.has('v')) return null; // a version built to try is the owner's
    if (req.method === 'GET' && p === '/' && hostApp) return { app: hostApp, page: true };
    let m = p.match(/^\/a\/([a-z0-9]+)\/?$/);
    if (req.method === 'GET' && m && this.published(m[1])) return { app: m[1], page: true };
    if (req.method === 'POST' && p === '/api/signout') return { app: hostApp };
    if (req.method === 'GET' && p === '/api/geocode') return { app: hostApp, dear: true };
    if (req.method === 'GET' && p === '/api/stream' && this.published(url.searchParams.get('app'))) return { app: url.searchParams.get('app'), stream: true };
    m = p.match(/^\/api\/apps\/([a-z0-9]+)(\/[a-z/]*)?$/);
    if (!m || !this.published(m[1])) return null;
    // On an app's own host, only that app.
    if (hostApp && hostApp !== m[1]) return null;
    const rest = m[2] || '';
    if (!APP_CALLS.some(([meth, r]) => meth === req.method && r === rest)) return null;
    return { app: m[1], call: rest, dear: DEAR.has(rest) };
  }

  // A visitor's rate: calls a minute, and fewer of the dear ones. null when
  // the call may go on, else seconds to wait.
  limit(req, dear) {
    const who = String(req.headers['x-forwarded-for'] || '').split(',')[0].trim() || req.socket?.remoteAddress || 'someone';
    const now = Date.now();
    let b = this.buckets.get(who);
    if (!b || now - b.at > 60000) { b = { calls: 0, dear: 0, at: now }; this.buckets.set(who, b); }
    if (this.buckets.size > 20000) for (const [k, x] of this.buckets) if (now - x.at > 60000) this.buckets.delete(k);
    b.calls++;
    if (dear) b.dear++;
    if (b.calls > this.rate.calls || (dear && b.dear > this.rate.dear)) return Math.ceil((60000 - (now - b.at)) / 1000);
    return null;
  }
}
